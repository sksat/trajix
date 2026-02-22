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
