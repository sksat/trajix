# trajix Design Doc

## Overview

trajix は、Android GNSS Logger アプリのログデータを可視化する Web アプリケーションである。

GNSS による移動履歴の表示に加え、**測位品質の詳細な分析**を主要な機能とする。精度、衛星配置、信号強度、コンステレーション別の品質を可視化し、測位品質が悪い区間では IMU データを用いた推測航法 (Dead Reckoning) で移動履歴を補完する。

ログファイルをブラウザに直接ドラッグ&ドロップし、すべての処理をクライアントサイドで完結させる。

## Data Format

### GNSS Logger 出力形式

GNSS Logger (Android) は CSV ベースのテキストファイルを出力する。各行はレコード型プレフィックスで始まり、カンマ区切りのフィールドが続く。

ファイル先頭にはヘッダ（`#` 始まりのコメント行）があり、デバイス情報と各レコード型のカラム定義が含まれる。

### レコード型

#### Fix（位置測位）

プロバイダ: GPS, FLP (Fused Location Provider), NLP (Network Location Provider)

```
Fix,Provider,LatitudeDegrees,LongitudeDegrees,AltitudeMeters,SpeedMps,
AccuracyMeters,BearingDegrees,UnixTimeMillis,SpeedAccuracyMps,
BearingAccuracyDegrees,elapsedRealtimeNanos,VerticalAccuracyMeters,
MockLocation,NumberOfUsedSignals,VerticalSpeedAccuracyMps,SolutionType
```

17 フィールド。NLP プロバイダでは altitude, speed, bearing 等が空になるケースがある。accuracy = 400.0 はフォールバック値。

#### Status（衛星ステータス）

```
Status,UnixTimeMillis,SignalCount,SignalIndex,ConstellationType,Svid,
CarrierFrequencyHz,Cn0DbHz,AzimuthDegrees,ElevationDegrees,UsedInFix,
HasAlmanacData,HasEphemerisData,BasebandCn0DbHz
```

14 フィールド。**UnixTimeMillis は常に空**であり、前後のレコードのタイムスタンプから推定する必要がある。

ConstellationType: GPS(1), GLONASS(3), QZSS(4), BeiDou(5), Galileo(6)

#### Raw（生 GNSS 測定値）

```
Raw,utcTimeMillis,TimeNanos,LeapSecond,...(54 fields total)
```

54 フィールド。衛星の ECEF 座標、擬似距離、搬送波位相、クロックバイアス、電離層補正 (Klobuchar) パラメータなど。QZSS 衛星では ECEF 座標が空になるケースがある (36,380 件)。

#### IMU センサー

| レコード型 | フィールド数 | 内容 |
|-----------|------------|------|
| UncalAccel | 10 | 未校正加速度 (x, y, z) + バイアス推定 |
| UncalGyro | 10 | 未校正角速度 (x, y, z) + ドリフト推定 |
| UncalMag | 10 | 未校正磁気 (x, y, z) + バイアス推定 |
| OrientationDeg | 7 | yaw, roll, pitch (オイラー角) |
| GameRotationVector | 7 | クォータニオン (x, y, z, w) |

#### その他

| レコード型 | 内容 |
|-----------|------|
| Nav | 航法メッセージ（バイナリデータ）。パース対象外 |
| Agc | 自動利得制御。CarrierFrequencyHz, AgcDb, ConstellationType |

## Architecture

### ブラウザ完結型アーキテクチャ

```
[ユーザー: .txt ファイルを D&D]
         |
         v
[Main Thread: React UI]
         |  File API で chunk 読み取り (4MB ずつ)
         |  postMessage(chunk) → Worker
         v
[Web Worker]
         |  WASM parser.feed(chunk)
         v
[WASM (trajix-wasm)]
         |  ストリーミングパーサ: 行ごとにパース
         |  パース結果を Apache Arrow RecordBatch に変換
         v
[Web Worker]
         |  Arrow RecordBatch を DuckDB-wasm に INSERT
         v
[DuckDB-wasm]
         |  テーブル: fix, status, raw, accel, gyro, mag, orientation, ...
         |  SQL で集計・フィルタ・時間範囲クエリ
         |  Dead Reckoning 用データの抽出も SQL
         v
[Main Thread: React UI]
         |  DuckDB から SQL でビュー用データを取得
         v
[CesiumJS / uPlot / SVG で描画]
```

### データフロー詳細

1. ユーザーがファイルを D&D
2. Main Thread が `FileReader` で 4MB チャンクずつ読み取り
3. 各チャンクを Web Worker に `postMessage` で送信
4. Web Worker が WASM の `feed(chunk)` を呼び出し
5. WASM がパース結果を Apache Arrow RecordBatch として返す
6. Web Worker が RecordBatch を DuckDB-wasm にバルク INSERT
7. パース完了後、UI が DuckDB に SQL クエリを発行してビュー用データを取得
8. React が取得データで各ビューを描画

### DuckDB テーブル設計

```sql
CREATE TABLE fix (
  provider VARCHAR,           -- 'GPS', 'FLP', 'NLP'
  latitude_deg DOUBLE,
  longitude_deg DOUBLE,
  altitude_m DOUBLE,          -- NULL for some NLP
  speed_mps DOUBLE,
  accuracy_m DOUBLE,
  bearing_deg DOUBLE,
  unix_time_ms BIGINT,
  speed_accuracy_mps DOUBLE,
  bearing_accuracy_deg DOUBLE,
  elapsed_realtime_ns BIGINT,
  vertical_accuracy_m DOUBLE,
  mock_location BOOLEAN,
  num_used_signals INTEGER,
  solution_type VARCHAR
);

CREATE TABLE status (
  unix_time_ms BIGINT,        -- inferred from neighboring records
  signal_count INTEGER,
  signal_index INTEGER,
  constellation INTEGER,      -- 1=GPS, 3=GLONASS, 4=QZSS, 5=BeiDou, 6=Galileo
  svid INTEGER,
  carrier_frequency_hz DOUBLE,
  cn0_dbhz DOUBLE,
  azimuth_deg DOUBLE,
  elevation_deg DOUBLE,
  used_in_fix BOOLEAN,
  has_almanac BOOLEAN,
  has_ephemeris BOOLEAN,
  baseband_cn0_dbhz DOUBLE
);

CREATE TABLE accel (
  utc_time_ms BIGINT,
  elapsed_realtime_ns BIGINT,
  x DOUBLE, y DOUBLE, z DOUBLE,
  bias_x DOUBLE, bias_y DOUBLE, bias_z DOUBLE,
  calibration_accuracy INTEGER
);

-- gyro, mag: same schema as accel (different column names)
-- orientation: utc_time_ms, yaw, roll, pitch, calibration_accuracy
-- game_rotation_vector: utc_time_ms, x, y, z, w
```

### クエリ例

```sql
-- 1秒ごとの平均 CN0（コンステレーション別）
SELECT
  (unix_time_ms / 1000) * 1000 AS epoch_ms,
  constellation,
  AVG(cn0_dbhz) AS avg_cn0,
  COUNT(*) AS sat_count,
  SUM(CASE WHEN used_in_fix THEN 1 ELSE 0 END) AS used_count
FROM status
GROUP BY epoch_ms, constellation
ORDER BY epoch_ms;

-- 精度が悪い区間の Fix
SELECT * FROM fix
WHERE provider = 'GPS' AND accuracy_m > 30
ORDER BY unix_time_ms;

-- スカイプロット用（特定エポックの衛星配置）
SELECT constellation, svid, azimuth_deg, elevation_deg, cn0_dbhz, used_in_fix
FROM status
WHERE unix_time_ms BETWEEN ? AND ?;
```

### コンポーネント構成

```
trajix/
  DESIGN.md
  Cargo.toml                    # Rust workspace
  crates/
    trajix/                    # パーサライブラリ (Rust)
      src/
        types.rs                # 共通型定義
        error.rs                # エラー型
        record/                 # レコード型 + パース
          fix.rs, status.rs, raw.rs, sensor.rs
        parser/                 # ストリーミングパーサ
          header.rs, line.rs, streaming.rs
        summary.rs              # 集計ロジック
        dead_reckoning.rs       # IMU 推測航法
    trajix-wasm/               # WASM バインディング
      src/lib.rs
  web/                          # TypeScript + React
    src/
      wasm/                     # WASM ローダー + Web Worker
      components/
        CesiumMap/              # 3D マップ
        SkyPlot/                # スカイプロット
        TimeSeries/             # 時系列チャート
        Constellation/          # コンステレーション別分析
        Fusion/                 # GNSS/IMU 融合ビュー
        PlaybackControls/       # 再生コントロール
```

## Technology Stack

| レイヤー | 技術 | 理由 |
|---------|------|------|
| パーサ | Rust | 1.2GB の高速処理。WASM 化可能 |
| WASM | wasm-bindgen + arrow-rs | Rust → Arrow RecordBatch → DuckDB |
| Web UI | React + TypeScript + Vite | |
| 分析 DB | DuckDB-wasm | パース結果を列指向 DB に格納。SQL で集計・フィルタ |
| 3D 地図 | CesiumJS + cesium-gsi-terrain | 国土地理院 DEM 地形 + PLATEAU 3D 建物 + アニメーション |
| チャート | uPlot | 高速時系列、小バンドル (~35KB) |
| スカイプロット | カスタム SVG | 極座標チャート |

## Visualization Design

### 位置信頼度モデル

メイントラック線とアニメーションには**最も信頼できる位置**を採用し、その不確実性を**信頼領域**として可視化する。ハード優先順位ではなく、**分散（variance）ベースのスコアリング**で最適なソースを動的に選択する。

位置計算はすべてローカル接平面座標（ENU: East-North-Up、メートル単位）で行う。

#### ソース別分散の推定

各位置ソースの分散（不確実性の二乗）を以下のように計算する。

**GNSS 分散**

入力: `accuracy_m`, `vertical_accuracy_m`, `num_used_signals`（Fix レコード）, `cn0_dbhz`（Status レコードの used_in_fix=true の平均 CN0）

```
定数: acc_floor=3m, acc_cap=200m, cn0_ref=35, cn0_min=20, n_ref=12, k_v=0.35

acc = clamp(accuracy_m, acc_floor, acc_cap)
v_acc = vertical_accuracy_m ?? acc * 1.5
n_used = max(num_used_signals ?? status_used_count, 1)
cn0_avg = max(avg_cn0_used ?? avg_cn0_top6, cn0_min)

cn0_factor = clamp((cn0_ref / cn0_avg) ^ 1.2, 0.7, 1.8)
n_factor   = clamp(sqrt(n_ref / n_used),       0.7, 1.8)

sigma_h = acc * cn0_factor * n_factor
sigma_3d = sqrt(sigma_h^2 + (k_v * v_acc)^2)
var_gnss = sigma_3d^2
```

`AccuracyMeters` は楽観的な場合がある（都市部のマルチパス等）ため、CN0 と衛星数で補正する。CN0 が低い / 衛星数が少ない場合は分散が大きくなる。

**Dead Reckoning 分散**

DR の位置誤差は加速度バイアスにより ~t² で成長する。

```
定数: drift_rate=0.02 m/s² (キャリブレーション前), sigma_heading=5°, sigma_speed=0.3 m/s

t = 最後の良好な GNSS Fix からの経過秒数
speed = max(speed_mps, 0.1)

sigma_bias    = 0.5 * drift_rate * t^2          // 加速度バイアスによる位置誤差
sigma_heading = speed * t * sigma_heading_rad    // heading ドリフトによる横方向誤差
sigma_speed   = sigma_speed * t                  // 速度誤差の蓄積

sigma_dr = sqrt(sigma_anchor^2 + sigma_bias^2 + sigma_heading^2 + sigma_speed^2)
var_dr = sigma_dr^2
```

`sigma_anchor` は DR 開始時点の GNSS sigma（不確実性の連続性を保証）。キャリブレーション後は `drift_rate` の代わりに `sigma_drift`（ドリフトレートのばらつき）を使うことで、補正済み DR の分散が小さくなる。

**FLP / NLP 分散**

報告された `accuracy_m` にプロバイダごとのペナルティ係数を乗算する。

```
sigma_flp = clamp(accuracy_m, 10, 200) * 1.3
sigma_nlp = clamp(accuracy_m, 50, 800) * 1.6
var_flp = sigma_flp^2
var_nlp = sigma_nlp^2
```

#### ソース選択: 最小分散 + ヒステリシス + 収束検出

各タイムスタンプで `argmin(variance)` を候補とするが、ちらつき防止のためヒステリシスを適用する。

**GNSS 復帰判定（収束検出ベース）**

GNSS 復帰後の安定は**固定秒数ではなく品質指標の収束**で判断する:
- 直近 N 個（3〜5 個）の連続 Fix で `var_gnss < var_active * 0.7`
- `accuracy_m` が減少傾向 or 安定（標準偏差が小さい）
- `cn0_avg >= 28` かつ `n_used >= 8`

```
gnss_stable = (
  consecutive_good_fixes >= 3
  AND stddev(recent_accuracy) < 3m
  AND all(var_gnss < var_active * 0.7 for recent fixes)
)
```

**GNSS 離脱判定**

```
gnss_unstable = (
  consecutive_bad_fixes >= 2
  AND (accuracy_m > 25 OR cn0_avg < 22 OR n_used < 5)
)
```

**最小滞在時間**: 切り替え後 3 秒間は再切り替えしない（高速トグル防止）。

#### スムーズな遷移

事後解析のため、切り替え前後に 3 秒のブレンドウィンドウを設ける:

```
t_s = 切り替え時刻
for t in [t_s - 1.5, t_s + 1.5]:
  alpha = clamp((t - (t_s - 1.5)) / 3.0, 0, 1)
  pos_display(t) = lerp(pos_old(t), pos_new(t), alpha)
```

#### 信頼領域の表示

位置の不確実性を視覚的に表現する:

- **GNSS 区間**: `sigma_3d` を半径とする半透明の円をトラックに沿って描画
- **Dead Reckoning 区間**: `sigma_dr` に基づき徐々に拡大するファンネル。キャリブレーション済みの場合は拡大速度が実特性に合致
- **垂直方向**: `VerticalAccuracyMeters` がある場合、高さ方向の不確実性も表示可能
- **表示切替**: 信頼領域の表示/非表示をトグルで制御

### 1. 3D マップビュー（CesiumJS）

メインビュー。国土地理院の 5m メッシュ DEM で地形を表示し、PLATEAU の 3D 建物データを重畳する。

- **メイントラック線**: 上記の位置信頼度モデルに基づき、最も信頼できる位置を結んだ線を描画
- トラックを `AccuracyMeters` で色分け（緑 < 5m → 黄 10m → 赤 > 50m、対数スケール）
- **信頼領域**: トラックに沿って半透明の帯 / 円を描画し、位置の不確実性を可視化
- Dead Reckoning 区間は破線で表示（信頼領域が徐々に広がる表現）
- **アニメーション**: Clock/Timeline でトラック上をマーカーが移動
  - 追従カメラ: マーカー後方から追従（Google Earth ツアー的体験）
  - 俯瞰カメラ: 上空から見下ろし
  - 切り替えボタンで 2 モードをトグル
- 再生コントロール: 再生/一時停止、速度調整 (1x, 5x, 10x, 50x)、シークバー

### 2. スカイプロット

極座標チャートで衛星配置を表示。

- 仰角を半径（天頂 = 中心、水平 = 外縁）、方位角を角度にマッピング
- CN0 で色分け、コンステレーション別マーカー形状
- UsedInFix の衛星をハイライト
- アニメーション時刻と連動して更新

### 3. 時系列チャート

複数のメトリクスを時間軸で表示。

- CN0 推移（全体平均 + コンステレーション別）
- 可視衛星数 / 使用衛星数
- 水平精度 / 垂直精度
- 速度（GNSS + IMU 推定の比較）
- IMU センサー値（加速度、角速度、姿勢角）
- 全チャートで連動するタイムカーソル

### 4. コンステレーション別分析

- CN0 分布の箱ひげ図
- UsedInFix 比率の比較
- コンステレーション別信号数の時間推移

### 5. GNSS/IMU 融合ビュー

- GNSS 軌跡と Dead Reckoning 軌跡の重ね合わせ（個別表示 + 融合軌跡）
- 位置信頼度モデルに基づく融合軌跡（メイントラックと同じベスト位置選択）
- 各ソースの信頼領域を同時表示（GNSS の AccuracyMeters 円 vs DR のドリフトファンネル）
- 信頼度スコアと IMU ドリフト量の時系列比較

### ビュー連動

**時間が全ビューの共通軸。** 3D マップのアニメーション時刻がスカイプロット・時系列チャートと完全同期する。時系列チャート上のクリックで 3D マップの位置もジャンプ。ブラシ選択で時間窓を設定すると全ビューが絞り込み。

## Dead Reckoning

GNSS 測位品質が悪い区間で IMU データから位置を推定する。

### 基本パイプライン

1. **姿勢推定**: GameRotationVector (quaternion) から端末の姿勢を取得
2. **座標変換**: 端末座標系の加速度 → 世界座標系 (ENU) に変換
3. **重力除去**: 加速度から重力成分を除去
4. **二重積分**: 加速度 → 速度 → 位置
5. **GNSS 融合**: 精度が閾値以下の区間で Dead Reckoning に切り替え、GNSS 復帰でリセット

### ドリフト対策

- 短時間（数十秒〜数分）に限定
- Zero Velocity Update (ZUPT): 静止検出で速度をリセット

### GNSS 復帰時キャリブレーション

IMU のドリフト特性はデバイスごとに異なるため、GNSS 測位が復帰したタイミングで Dead Reckoning の推定結果と実際の GNSS 位置を比較し、IMU パラメータを補正する。

#### 仕組み

1. **誤差計測**: DR 区間終了時（GNSS 復帰・収束確認後）に、DR 推定位置と GNSS Fix の差分 `E`（位置誤差）および `V`（速度誤差、利用可能な場合）を記録
2. **ドリフトレート推定**: 複数の DR 区間から加速度バイアスの大きさを推定
   - 各区間: `b_i = 2 * |E_i| / T_i^2`
   - ロバスト推定: `drift_rate = median(b_i)`, `sigma_drift = 1.4826 * MAD(b_i)`
   - EWMA による逐次更新: `drift_rate = 0.8 * drift_rate + 0.2 * b_i`
3. **二次後方補正 (quadratic backward correction)**: 誤差は ~t² で成長するため、二次曲線で補正（線形では不十分）

```
// 位置誤差のみの場合
err(t) = E * (t / T)^2
p_corrected(t) = p_dr(t) - err(t)

// 速度誤差も利用可能な場合（より正確）
b  = 2 * (E - V*T) / T^2
v0 = V - b*T
err(t) = v0*t + 0.5*b*t^2
p_corrected(t) = p_dr(t) - err(t)
```

4. **分散モデルへの反映**: 補正済み DR では `drift_rate` の代わりに `sigma_drift`（ドリフトレートのばらつき）を使用。キャリブレーションが安定しているほど補正済み DR の分散が小さくなる

```
sigma_bias_corrected = 0.5 * sigma_drift * t^2
var_dr_corrected = sigma_anchor^2 + sigma_bias_corrected^2 + sigma_heading^2 + ...
```

#### heading ドリフト対策

`GameRotationVector` は磁気参照なしのため heading が大きくずれうる。以下で対策:
- **GNSS speed/course による yaw 再アンカー**: 移動中（speed > 1 m/s）の GNSS bearing を使って heading を補正
- **DR を数十秒に限定**: heading ドリフトが支配的になる前に他のソースにフォールバック

#### 2 パス処理

事後解析のためファイル全体を 2 パスで処理:

```
Pass 1: キャリブレーション情報収集
  - 全 DR 区間を特定（GNSS 良好→劣化→復帰）
  - 各区間の E_i, T_i を計測
  - drift_rate, sigma_drift を推定

Pass 2: 補正適用
  - 二次後方補正を全 DR 区間に適用
  - 補正済みの分散で位置信頼度モデルのスコアリングを実行
```

#### 制約

- 初回の DR 区間はキャリブレーション未実施（デバイス報告のバイアスのみ使用）
- 十分な GNSS 復帰イベント（3 回以上）が無いとドリフトレート推定の信頼性が低い
- ファイル末尾の DR 区間は GNSS 復帰がないため後方補正不可（生の DR + デフォルト分散を使用）

## Browser Memory Budget

DuckDB-wasm が列指向でデータを保持するため、メモリ効率が良い。ビュー用のクエリ結果のみが JS ヒープに展開される。

| データ | DuckDB 内サイズ (推定) |
|-------|----------------------|
| fix (51K rows, 15 cols) | ~5MB |
| status (478K rows, 13 cols) | ~30MB |
| accel/gyro/mag/orientation/rotation | ダウンサンプリング後: ~10MB |
| **DuckDB 合計** | **~45MB** |
| **JS ヒープ (ビュー用クエリ結果)** | **~5-10MB** |

生データ全量をパースして DuckDB に格納するが、1.2GB のテキストが列指向圧縮で ~45MB に収まる。ビューが必要とするデータだけを SQL でフェッチするため、JS ヒープの消費は最小限。

## Downsampling

高頻度センサーデータ（~100Hz、各 2.3M レコード）と広域表示時のチャートデータを効率的に間引く。

### データ種別ごとの戦略

| データ種別 | レコード数 | 戦略 |
|-----------|-----------|------|
| Fix | 51K | 全件保持（少量） |
| Status | 478K | 全件保持（スカイプロットに必要） |
| Raw | 477K | 全件保持（CN0 集計に必要） |
| IMU センサー | 各 ~2.3M | 時間ベース間引き: 100Hz → 10Hz（表示用）。DR 計算は全件使用 |
| チャート描画 | 可変 | LTTB でズームレベルに応じて動的に間引き |

### アルゴリズム

#### 1. 時間ベース間引き (Temporal Decimation)

固定グリッドビン方式。最初のサンプルのタイムスタンプを基準に `interval_ms` 間隔のグリッドを敷き、各ビン内でビン中心に最も近いサンプルを 1 つ選択する。

```
grid: [t0, t0+dt), [t0+dt, t0+2dt), [t0+2dt, t0+3dt), ...
各ビン: center = t0 + (n+0.5)*dt に最も近いサンプルを選択
先頭・末尾は常に保持
```

**固定グリッドを採用する理由**: サンプル間隔からの相対境界（前のサンプルから dt 後）だと、不規則なサンプリングでドリフトが蓄積する。固定グリッドは時間軸上で均一な間隔を保証する。

用途: IMU データの 100Hz → 10Hz 変換（`interval_ms = 100`）。

#### 2. LTTB (Largest Triangle Three Buckets)

視覚的に重要な点を優先的に保持するダウンサンプリング。データを `target_count` 個のバケットに分割し、各バケットで隣接バケットとの三角形面積が最大となる点を選択する。

特性:
- ピーク・谷・急変を優先的に保持（信号の形状を維持）
- 入力のサブセットを出力（補間なし）
- 先頭・末尾は常に保持

用途: 時系列チャートのズームアウト時。例: 2.3M → 2000 点（画面幅ピクセル相当）。

#### 3. 多軸データの LTTB

3 軸センサーデータ（加速度計 X/Y/Z 等）では、**軸ごとに独立して LTTB を実行すると時刻がずれる**。代わりに:

1. L2 ノルム（magnitude）でサンプル列を作成: `mag_i = sqrt(x_i² + y_i² + z_i²)`
2. magnitude に対して LTTB を実行し、選択された**インデックス列**を取得 (`lttb_indices`)
3. 同じインデックスで全軸のデータを選択 → 全軸が時刻整合

これにより軸固有のスパイクが magnitude の大きさに反映され、キャンセレーション（+X と -Y が相殺して magnitude が変わらない）のリスクを最小化する。

### 処理タイミング

バッチ処理（フルパース後）で実行。ストリーミング中のインクリメンタルダウンサンプリングは不要（アプリはフルパース完了後に利用可能になるため）。

```
パース完了 → DuckDB 格納 → ビュー描画時に SQL + ダウンサンプリング
                                 ├── ズームイン: 生データ表示
                                 ├── 標準: 10Hz データ
                                 └── ズームアウト: LTTB で画面幅に合わせて間引き
```
