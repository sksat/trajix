use std::io::BufRead;

use crate::error::ParseError;
use crate::parser::header::HeaderInfo;
use crate::parser::line::{Record, parse_line};

/// Streaming parser for GNSS Logger files.
///
/// Reads lines from a `BufRead` source, parses the header, then
/// yields parsed `Record` values via iteration.
pub struct StreamingParser<R> {
    reader: R,
    line_buf: String,
    line_number: u64,
    header: Option<HeaderInfo>,
    header_lines: Vec<String>,
    header_parsed: bool,
}

impl<R: BufRead> StreamingParser<R> {
    pub fn new(reader: R) -> Self {
        StreamingParser {
            reader,
            line_buf: String::new(),
            line_number: 0,
            header: None,
            header_lines: Vec::new(),
            header_parsed: false,
        }
    }

    /// The parsed header info (available after iterating past header lines).
    pub fn header(&self) -> Option<&HeaderInfo> {
        self.header.as_ref()
    }

    /// Current line number (1-based).
    pub fn line_number(&self) -> u64 {
        self.line_number
    }

    /// Read the next record, skipping comments and blank lines.
    ///
    /// Returns `None` at EOF.
    fn next_record(&mut self) -> Option<Result<Record, ParseError>> {
        loop {
            self.line_buf.clear();
            match self.reader.read_line(&mut self.line_buf) {
                Ok(0) => {
                    // EOF — finalize header if not yet done
                    self.finalize_header();
                    return None;
                }
                Ok(_) => {
                    self.line_number += 1;

                    // Collect header comment lines
                    if self.line_buf.starts_with('#') {
                        let line = self.line_buf.trim_end().to_string();
                        self.header_lines.push(line);
                        continue;
                    }

                    // First non-comment line: finalize header
                    self.finalize_header();

                    let result = parse_line(self.line_buf.trim_end());

                    match result {
                        None => continue, // blank line
                        Some(Ok(Record::Skipped)) => continue,
                        Some(Ok(record)) => return Some(Ok(record)),
                        Some(Err(e)) => {
                            return Some(Err(ParseError::AtLine {
                                line_number: self.line_number,
                                source: Box::new(e),
                            }));
                        }
                    }
                }
                Err(e) => {
                    return Some(Err(ParseError::FieldParse {
                        field: "line_read",
                        source: Box::new(e),
                    }));
                }
            }
        }
    }

    fn finalize_header(&mut self) {
        if !self.header_parsed {
            self.header_parsed = true;
            let refs: Vec<&str> = self.header_lines.iter().map(|s| s.as_str()).collect();
            self.header = HeaderInfo::parse(&refs);
        }
    }
}

impl<R: BufRead> Iterator for StreamingParser<R> {
    type Item = Result<Record, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_record()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FixProvider;

    fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn stream_header_only() {
        let content = load_fixture("header.txt");
        let mut parser = StreamingParser::new(content.as_bytes());

        // Drain all records (there should be none — header-only file)
        let records: Vec<_> = parser.by_ref().collect();
        assert!(records.is_empty());

        // Header should be parsed
        let header = parser.header().expect("header should be parsed");
        assert_eq!(header.version, "v3.1.1.2");
        assert_eq!(header.model, "SH-M26");
    }

    #[test]
    fn stream_mixed_records() {
        let content = load_fixture("mixed_records.txt");
        let parser = StreamingParser::new(content.as_bytes());

        let records: Vec<_> = parser.map(|r| r.expect("should parse")).collect();

        // mixed_records.txt has 100 data lines (no skipped types)
        assert_eq!(records.len(), 100);
    }

    #[test]
    fn stream_header_then_data() {
        // Combine header + data
        let header = load_fixture("header.txt");
        let data = load_fixture("fix_normal_gps.txt");
        let combined = format!("{header}{data}");

        let mut parser = StreamingParser::new(combined.as_bytes());
        let records: Vec<_> = parser.by_ref().collect();

        // All Fix records should parse
        assert!(!records.is_empty());
        for r in &records {
            match r.as_ref().unwrap() {
                Record::Fix(f) => assert_eq!(f.provider, FixProvider::Gps),
                _ => panic!("expected Fix records"),
            }
        }

        // Header should be available
        let header = parser.header().expect("header should be parsed");
        assert_eq!(header.manufacturer, "SHARP");
    }

    #[test]
    fn stream_tracks_line_numbers() {
        let content = load_fixture("mixed_records.txt");
        let mut parser = StreamingParser::new(content.as_bytes());

        // Read first record
        let _first = parser.next().unwrap().unwrap();
        assert!(parser.line_number() >= 1);

        // Drain remaining
        let count = parser.count();
        assert!(count > 0);
    }

    #[test]
    fn stream_preserves_record_order() {
        let content = load_fixture("mixed_records.txt");
        let parser = StreamingParser::new(content.as_bytes());

        let records: Vec<_> = parser.map(|r| r.unwrap()).collect();

        // First 6 should be Status (from fixture)
        for r in &records[..6] {
            assert!(matches!(r, Record::Status(_)));
        }

        // Record at index 6 should be UncalMag
        assert!(matches!(records[6], Record::UncalMag(_)));
    }

    #[test]
    fn stream_record_type_variety() {
        let content = load_fixture("mixed_records.txt");
        let parser = StreamingParser::new(content.as_bytes());

        let mut has_fix = false;
        let mut has_status = false;
        let mut has_raw = false;
        let mut has_sensor = false;

        for record in parser {
            match record.unwrap() {
                Record::Fix(_) => has_fix = true,
                Record::Status(_) => has_status = true,
                Record::Raw(_) => has_raw = true,
                Record::UncalAccel(_)
                | Record::UncalGyro(_)
                | Record::UncalMag(_)
                | Record::OrientationDeg(_)
                | Record::GameRotationVector(_) => has_sensor = true,
                Record::Skipped => {}
            }
        }

        assert!(has_fix);
        assert!(has_status);
        assert!(has_raw);
        assert!(has_sensor);
    }
}
