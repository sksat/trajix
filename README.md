# trajix

GNSS/positioning data visualization web app. Parses 1GB+ [Android GNSS Logger](https://play.google.com/store/apps/details?id=com.google.android.apps.location.gps.gnsslogger) log files in-browser via WASM, and visualizes flight trajectories on 3D maps with sky plots and time-series charts.

https://github.com/user-attachments/assets/c2eea891-5479-4f58-992d-862c72e3b26c

*Demo: Chitose → Narita flight (2025-11-29), 607MB log file parsed in ~10s*

## Features

- **In-browser WASM parser** — streams 1GB+ files without server upload
- **3D flight visualization** — CesiumJS with GSI terrain tiles, camera follow mode
- **Sky plot** — real-time satellite positions with constellation filtering
- **Time-series charts** — CN0, satellite count, accuracy, speed (uPlot)
- **DuckDB-wasm** — SQL queries on parsed data in-browser

## Usage (Rust library)

```rust
use std::fs::File;
use std::io::BufReader;
use trajix::parser::streaming::StreamingParser;
use trajix::parser::line::Record;

let file = File::open("gnss_log.txt").unwrap();
let reader = BufReader::new(file);
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
