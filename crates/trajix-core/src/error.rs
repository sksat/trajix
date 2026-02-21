use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unknown record type: {0}")]
    UnknownRecordType(String),

    #[error("wrong number of fields: expected {expected}, got {actual} (record type: {record_type})")]
    FieldCount {
        record_type: &'static str,
        expected: usize,
        actual: usize,
    },

    #[error("failed to parse field '{field}': {source}")]
    FieldParse {
        field: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("unknown provider: {0}")]
    UnknownProvider(String),

    #[error("unknown constellation type: {0}")]
    UnknownConstellation(u8),

    #[error("line {line_number}: {source}")]
    AtLine {
        line_number: u64,
        source: Box<ParseError>,
    },
}
