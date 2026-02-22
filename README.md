# trajix

**https://sksat.github.io/trajix/**

GNSS/positioning data visualization web app. Parses 1GB+ [Android GNSS Logger](https://play.google.com/store/apps/details?id=com.google.android.apps.location.gps.gnsslogger) log files in-browser via WASM, and visualizes flight trajectories on 3D maps with sky plots and time-series charts.

<table>
  <tr>
    <td><img src="https://github.com/user-attachments/assets/10090900-729f-4587-a1f0-370c2540fdf0" alt="Chitose → Narita flight" width="225" height="400"></td>
    <td><img src="https://github.com/user-attachments/assets/2308af1a-c862-4e3a-9528-cfd9cb6116b9" alt="Mt. Tsukuba ascent" width="225" height="400"></td>
    <td><img src="https://github.com/user-attachments/assets/53c17d3a-3681-487b-a255-030334940db7" alt="Mt. Tsukuba descent" width="225" height="400"></td>
  </tr>
  <tr>
    <td><em>Chitose → Narita flight</em></td>
    <td><em>Mt. Tsukuba ascent</em></td>
    <td><em>Mt. Tsukuba descent</em></td>
  </tr>
</table>

## Features

- **In-browser WASM parser** — streams 1GB+ files without server upload
- **3D flight visualization** — CesiumJS with GSI terrain tiles, camera follow mode
- **Sky plot** — real-time satellite positions with constellation filtering
- **Time-series charts** — CN0, satellite count, accuracy, speed (uPlot)
- **DuckDB-wasm** — SQL queries on parsed data in-browser

## Rust library

### Quick start

```rust
use trajix::prelude::*;

let file = std::fs::File::open("gnss_log.txt").unwrap();
let reader = std::io::BufReader::new(file);
let mut parser = StreamingParser::new(reader);

for result in &mut parser {
    match result {
        Ok(Record::Fix(fix)) => {
            println!("{}: ({}, {})",
                fix.provider, fix.latitude_deg, fix.longitude_deg);
        }
        Ok(_) => {} // Status, Raw, IMU sensors, etc.
        Err(e) => eprintln!("parse error: {e}"),
    }
}

if let Some(header) = parser.header() {
    println!("{} {}", header.manufacturer, header.model);
}
```

### Extract fixes with iterator extensions

```rust
use trajix::prelude::*;

let file = std::fs::File::open("gnss_log.txt").unwrap();
let parser = StreamingParser::new(std::io::BufReader::new(file));

// Collect only GPS+FLP fixes (skip NLP)
let fixes: Vec<FixRecord> = parser.primary_fixes().collect();

println!("{} primary fixes", fixes.len());
```

### Distance, speed, and statistics

```rust
use trajix::FixRecord;
use trajix::stats::summarize_fixes;

fn analyze(fixes: &[FixRecord]) {
    // Distance and speed between two fixes
    let dist = fixes[0].distance_to(&fixes[1]); // meters
    let dt = fixes[0].time_delta_s(&fixes[1]);   // seconds
    let speed = fixes[0].speed_between(&fixes[1]); // Option<f64> m/s

    // Summary statistics
    let stats = summarize_fixes(fixes);
    println!("Duration: {:.0}s", stats.duration_s);
    println!("Distance: {:.0}m", stats.total_distance_m);
    if let Some(acc) = &stats.accuracy {
        println!("Accuracy: median={:.1}m, p95={:.1}m", acc.median, acc.p95);
    }
}
```

### Fix quality classification

```rust
use trajix::quality::{classify_fixes, FixQuality, DEFAULT_GAP_THRESHOLD_MS};
use trajix::FixRecord;

fn classify(fixes: &[FixRecord]) {
    let qualities = classify_fixes(fixes, DEFAULT_GAP_THRESHOLD_MS);
    for (fix, quality) in fixes.iter().zip(&qualities) {
        match quality {
            FixQuality::Primary => { /* GPS/FLP - use for track */ }
            FixQuality::GapFallback => { /* NLP during GPS gap */ }
            FixQuality::Rejected => { /* NLP redundant with GPS */ }
        }
    }
}
```

### Geodesic utilities

```rust
use trajix::geo::{haversine_distance_m, bearing_deg};

let dist = haversine_distance_m(35.6812, 139.7671, 34.7024, 135.4959);
println!("Tokyo to Osaka: {:.0} km", dist / 1000.0);

let bearing = bearing_deg(35.6812, 139.7671, 34.7024, 135.4959);
println!("Bearing: {:.1}°", bearing);
```

## Modules

| Module | Description |
|--------|-------------|
| `parser` | Streaming CSV parser (`StreamingParser`), line parser (`parse_line`), iterator filters |
| `record` | Typed record structs: `FixRecord`, `StatusRecord`, `RawRecord`, sensor records |
| `types` | Core enums: `ConstellationType`, `FixProvider`, `RecordType`, `CodeType` |
| `geo` | Geodesic utilities: `haversine_distance_m`, `bearing_deg` |
| `quality` | Fix quality classification: `FixQuality`, `classify_fixes` |
| `stats` | Statistical summaries: `summarize_fixes`, `PercentileStats` |
| `summary` | Epoch aggregation: `EpochAggregator`, `StatusEpoch`, `FixEpoch` |
| `downsample` | Time-series reduction: `decimate_by_time`, `lttb`, `StreamingDecimator` |
| `dead_reckoning` | IMU-based Dead Reckoning for GNSS-degraded segments |

## Record types

| Type | Description |
|------|-------------|
| `Fix` | GPS/FLP/NLP position fixes with accuracy, speed, bearing |
| `Status` | Satellite visibility and signal strength per constellation |
| `Raw` | Raw GNSS measurements (pseudorange, carrier phase, Doppler) |
| `UncalAccel` / `UncalGyro` / `UncalMag` | Uncalibrated IMU sensors |
| `OrientationDeg` | Device orientation (azimuth, pitch, roll) |
| `GameRotationVector` | Fused rotation quaternion |

## License

[MPL-2.0](LICENSE)
