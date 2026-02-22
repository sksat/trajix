//! GNSS/positioning data parser and analysis library.
//!
//! Currently supports Android GNSS Logger CSV format, parsing Fix, Status, Raw,
//! and IMU sensor records into strongly-typed Rust structs with streaming support
//! for large (1GB+) files.
//!
//! # Quick start
//!
//! ```no_run
//! use trajix::prelude::*;
//!
//! let file = std::fs::File::open("gnss_log.txt").unwrap();
//! let reader = std::io::BufReader::new(file);
//! let mut parser = StreamingParser::new(reader);
//!
//! for result in &mut parser {
//!     match result {
//!         Ok(Record::Fix(fix)) => {
//!             println!("{}: ({}, {})", fix.provider, fix.latitude_deg, fix.longitude_deg);
//!         }
//!         Ok(_) => {}
//!         Err(e) => eprintln!("parse error: {e}"),
//!     }
//! }
//! ```

pub mod dead_reckoning;
pub mod downsample;
pub mod error;
pub mod geo;
pub mod parser;
pub mod quality;
pub mod record;
pub mod stats;
pub mod summary;
pub mod types;

// Top-level re-exports for ergonomic imports.
pub use error::ParseError;
pub use parser::filter::FilterRecords;
pub use parser::header::HeaderInfo;
pub use parser::line::{parse_line, Record};
pub use parser::streaming::StreamingParser;
pub use record::fix::FixRecord;
pub use record::raw::RawRecord;
pub use record::sensor::{GameRotationVectorRecord, OrientationRecord, UncalibratedSensorRecord};
pub use record::status::StatusRecord;
pub use quality::FixQuality;
pub use summary::{ConstellationStats, EpochAggregator, FixEpoch, StatusEpoch};
pub use types::{CodeType, ConstellationType, FixProvider, RecordType};

/// Convenience re-exports for common usage patterns.
///
/// ```
/// use trajix::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        ConstellationType, FilterRecords, FixProvider, FixRecord, ParseError, Record, RecordType,
        StatusRecord, StreamingParser,
    };
}
