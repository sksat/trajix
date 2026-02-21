//! GNSS/positioning data parser and analysis library.
//!
//! Currently supports Android GNSS Logger CSV format, parsing Fix, Status, Raw,
//! and IMU sensor records into strongly-typed Rust structs with streaming support
//! for large (1GB+) files.

pub mod dead_reckoning;
pub mod downsample;
pub mod error;
pub mod parser;
pub mod record;
pub mod summary;
pub mod types;
