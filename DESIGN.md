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
    trajix-core/               # パーサライブラリ (Rust)
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

メイントラック線とアニメーションには**最も信頼できる位置**を採用し、その不確実性を**信頼領域**として可視化する。

#### ベスト位置の選択ロジック

各時刻で利用可能な位置ソースから最も信頼できるものを選択する:

1. **GPS Fix（AccuracyMeters ≤ 閾値）**: 最優先。精度が良い GNSS 測位
2. **Dead Reckoning 推定位置**: GNSS 精度が劣化した区間（AccuracyMeters > 閾値、または Fix 欠損）で IMU ベースの推定位置を使用
3. **FLP Fix**: GPS Fix が無い場合のフォールバック（Fused Location Provider）
4. **NLP Fix**: 最後の手段（Network Location Provider、精度 ~400m）

閾値の目安: AccuracyMeters > 30m で Dead Reckoning に切り替え。Dead Reckoning の累積ドリフトが一定値を超えた場合は FLP/NLP にフォールバック。

#### 信頼領域の表示

位置の不確実性を視覚的に表現する:

- **GNSS 区間**: `AccuracyMeters` を半径とする半透明の円をトラックに沿って描画。精度が良い区間は小さく薄い円、悪い区間は大きく濃い円
- **Dead Reckoning 区間**: 時間経過とともに信頼領域が徐々に拡大する（ドリフト蓄積を反映）。扇状に広がるファンネル表現。拡大速度はキャリブレーション済みのドリフトレートに基づく（後述「GNSS 復帰時キャリブレーション」参照）
- **垂直方向**: `VerticalAccuracyMeters` がある場合、高さ方向の不確実性も表示可能（3D 楕円体 or 垂直バー）
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

1. **誤差計測**: Dead Reckoning 区間の終了時（GNSS 復帰時）に、DR 推定位置と GNSS Fix の差分（位置誤差・速度誤差）を記録
2. **バイアス学習**: 複数の DR→GNSS 復帰イベントから、加速度バイアスの残差傾向を学習
   - `UncalAccel` の `BiasX/Y/Z` はデバイス自身の推定だが、実測との乖離を補正項として蓄積
   - 時間経過に伴うドリフトレート（m/s²/s）を推定
3. **後方補正 (backward correction)**: GNSS 復帰時の誤差を DR 区間全体に按分して逆補正
   - 線形補間: 区間始点（誤差 0）から終点（実測誤差）への線形分配
   - 後のイベントほどキャリブレーション精度が向上
4. **信頼領域への反映**: キャリブレーション済みのドリフトレートを使って、信頼領域の拡大速度をデバイスの実特性に合わせる

#### データフロー

```
[GNSS 良好区間] → DR 開始 → [DR 区間] → GNSS 復帰
      │                         │              │
      │                         │         誤差計測 (DR推定 vs GNSS)
      │                         │              │
      │                    後方補正 ←──── 誤差分配
      │                         │
      └── バイアス/ドリフトレート学習 ← 複数イベントで蓄積
```

#### 制約

- 初回の DR 区間はキャリブレーション未実施（デバイス報告のバイアスのみ使用）
- 十分な GNSS 復帰イベント（3 回以上）が無いとドリフトレート推定の信頼性が低い
- ログは事後解析なので、ファイル全体を 2 パスで処理可能（1 パス目: キャリブレーション情報収集、2 パス目: 補正適用）

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
