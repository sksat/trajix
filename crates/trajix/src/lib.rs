//! Parser library for Android GNSS Logger CSV format.
//!
//! Parses Fix, Status, Raw, and IMU sensor records from GNSS Logger log files
//! into strongly-typed Rust structs. Supports streaming parsing for large files.

pub mod dead_reckoning;
pub mod downsample;
pub mod error;
pub mod parser;
pub mod record;
pub mod summary;
pub mod types;
