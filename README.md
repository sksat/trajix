# trajix

GNSS/positioning data parser and analysis library.

Currently supports [Android GNSS Logger](https://play.google.com/store/apps/details?id=com.google.android.apps.location.gps.gnsslogger) CSV format, parsing Fix, Status, Raw, and IMU sensor records into strongly-typed Rust structs with streaming support for large (1GB+) files.

## Usage

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
