use crate::error::ParseError;
use crate::parser::line::Record;
use crate::record::fix::FixRecord;
use crate::types::{FixProvider, RecordType};

/// Iterator adapter that keeps only records of specified types.
pub struct TypeFilter<I> {
    inner: I,
    /// Bitfield of allowed record types. Bit N corresponds to RecordType discriminant N.
    allowed: u16,
    /// Whether to keep Skipped records (which have no RecordType).
    keep_skipped: bool,
}

impl<I> TypeFilter<I> {
    fn new(inner: I, types: &[RecordType], keep_skipped: bool) -> Self {
        let mut allowed = 0u16;
        for &rt in types {
            allowed |= 1 << (rt as u16);
        }
        TypeFilter {
            inner,
            allowed,
            keep_skipped,
        }
    }

    fn is_allowed(&self, record: &Record) -> bool {
        match record.record_type() {
            Some(rt) => self.allowed & (1 << (rt as u16)) != 0,
            None => self.keep_skipped,
        }
    }
}

impl<I> Iterator for TypeFilter<I>
where
    I: Iterator<Item = Result<Record, ParseError>>,
{
    type Item = Result<Record, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok(record) => {
                    if self.is_allowed(&record) {
                        return Some(Ok(record));
                    }
                    // Skip this record, continue to next
                }
                err @ Err(_) => return Some(err),
            }
        }
    }
}

/// Iterator adapter that keeps only records within a time range.
///
/// Records without timestamps (Skipped, Status with empty time) are always passed through.
pub struct TimeFilter<I> {
    inner: I,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}

impl<I> TimeFilter<I> {
    fn new(inner: I, start_ms: Option<i64>, end_ms: Option<i64>) -> Self {
        TimeFilter {
            inner,
            start_ms,
            end_ms,
        }
    }

    fn is_in_range(&self, record: &Record) -> bool {
        match record.timestamp_ms() {
            Some(ts) => {
                if let Some(start) = self.start_ms
                    && ts < start
                {
                    return false;
                }
                if let Some(end) = self.end_ms
                    && ts > end
                {
                    return false;
                }
                true
            }
            // Records without timestamps pass through
            None => true,
        }
    }
}

impl<I> Iterator for TimeFilter<I>
where
    I: Iterator<Item = Result<Record, ParseError>>,
{
    type Item = Result<Record, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok(record) => {
                    if self.is_in_range(&record) {
                        return Some(Ok(record));
                    }
                }
                err @ Err(_) => return Some(err),
            }
        }
    }
}

/// Extension trait to add filtering to any record iterator.
pub trait FilterRecords: Iterator<Item = Result<Record, ParseError>> + Sized {
    /// Keep only records of the specified types. Skipped records are dropped.
    fn filter_types(self, types: &[RecordType]) -> TypeFilter<Self> {
        TypeFilter::new(self, types, false)
    }

    /// Keep only records of the specified types, plus Skipped records.
    fn filter_types_keep_skipped(self, types: &[RecordType]) -> TypeFilter<Self> {
        TypeFilter::new(self, types, true)
    }

    /// Keep only records within the given time range (inclusive).
    ///
    /// `None` for start or end means unbounded on that side.
    /// Records without timestamps (e.g. Skipped) always pass through.
    fn filter_time(self, start_ms: Option<i64>, end_ms: Option<i64>) -> TimeFilter<Self> {
        TimeFilter::new(self, start_ms, end_ms)
    }

    /// Extract only [`FixRecord`]s, discarding all other record types and errors.
    fn fixes(self) -> FixExtractor<Self> {
        FixExtractor { inner: self }
    }

    /// Extract only primary (GPS/FLP) [`FixRecord`]s, discarding NLP and other types.
    fn primary_fixes(self) -> PrimaryFixExtractor<Self> {
        PrimaryFixExtractor { inner: self }
    }
}

impl<I> FilterRecords for I where I: Iterator<Item = Result<Record, ParseError>> {}

/// Iterator adapter that extracts [`FixRecord`]s from a record stream.
pub struct FixExtractor<I> {
    inner: I,
}

impl<I> Iterator for FixExtractor<I>
where
    I: Iterator<Item = Result<Record, ParseError>>,
{
    type Item = FixRecord;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok(Record::Fix(fix)) => return Some(fix),
                Ok(_) | Err(_) => continue,
            }
        }
    }
}

/// Iterator adapter that extracts primary (GPS/FLP) [`FixRecord`]s.
pub struct PrimaryFixExtractor<I> {
    inner: I,
}

impl<I> Iterator for PrimaryFixExtractor<I>
where
    I: Iterator<Item = Result<Record, ParseError>>,
{
    type Item = FixRecord;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next()? {
                Ok(Record::Fix(fix))
                    if fix.provider == FixProvider::Gps || fix.provider == FixProvider::Flp =>
                {
                    return Some(fix);
                }
                Ok(_) | Err(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::line::parse_line;

    fn parse_lines(text: &str) -> Vec<Result<Record, ParseError>> {
        text.lines().filter_map(parse_line).collect()
    }

    #[test]
    fn filter_fix_only() {
        let input = "\
Fix,GPS,36.2122343400,140.0985108900,284.0,0.91,9.9,112.1,1771641936000,0.82,25.9,2092092474651730,2.8,0,,,
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_types(&[RecordType::Fix])
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(filtered.len(), 1);
        assert!(matches!(filtered[0], Record::Fix(_)));
    }

    #[test]
    fn filter_multiple_types() {
        let input = "\
Fix,GPS,36.2122343400,140.0985108900,284.0,0.91,9.9,112.1,1771641936000,0.82,25.9,2092092474651730,2.8,0,,,
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_types(&[RecordType::Fix, RecordType::Status])
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(filtered.len(), 2);
        assert!(matches!(filtered[0], Record::Fix(_)));
        assert!(matches!(filtered[1], Record::Status(_)));
    }

    #[test]
    fn filter_skipped_records() {
        let input = "\
Fix,GPS,36.2122343400,140.0985108900,284.0,0.91,9.9,112.1,1771641936000,0.82,25.9,2092092474651730,2.8,0,,,
Nav,1,2,3,4,5,data";

        let records = parse_lines(input);

        // Without keep_skipped: Nav is dropped
        let filtered: Vec<_> = records
            .into_iter()
            .filter_types(&[RecordType::Fix])
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(filtered.len(), 1);

        // With keep_skipped: Nav passes through as Skipped
        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_types_keep_skipped(&[RecordType::Fix])
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_time_range() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641935000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_time(Some(1771641932000), Some(1771641937000))
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].timestamp_ms(), Some(1771641935000));
    }

    #[test]
    fn filter_time_open_start() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641935000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_time(None, Some(1771641935000))
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_time_open_end() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641935000,,,2092092474651730,2.8,0,,,
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_time(Some(1771641935000), None)
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_time_passes_no_timestamp_records() {
        // Status without timestamp should pass through time filter
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Status,,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_time(Some(1771641935000), Some(1771641945000))
            .map(|r| r.unwrap())
            .collect();

        // First Fix filtered out, Status passes through (no timestamp), last Fix passes
        assert_eq!(filtered.len(), 2);
        assert!(matches!(filtered[0], Record::Status(_)));
        assert!(matches!(filtered[1], Record::Fix(_)));
    }

    #[test]
    fn composable_filters() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641935000,,,2092092474651730,2.8,0,,,
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let filtered: Vec<_> = records
            .into_iter()
            .filter_types(&[RecordType::Fix])
            .filter_time(Some(1771641932000), Some(1771641937000))
            .map(|r| r.unwrap())
            .collect();

        // Only the Fix at 1771641935000 matches both filters
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].timestamp_ms(), Some(1771641935000));
    }

    // ──────────────────────────────────────────────
    // fixes() / primary_fixes() tests
    // ──────────────────────────────────────────────

    #[test]
    fn fixes_extracts_only_fix_records() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Fix,FLP,36.213,140.098,285.0,1.0,5.0,,1771641935000,,,2092092474651730,2.8,0,,,
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3
Fix,NLP,36.500,140.500,,,400.0,,1771641940000,,,,,,,,";

        let records = parse_lines(input);
        let fixes: Vec<_> = records.into_iter().fixes().collect();

        assert_eq!(fixes.len(), 3);
        assert_eq!(fixes[0].provider, crate::types::FixProvider::Gps);
        assert_eq!(fixes[1].provider, crate::types::FixProvider::Flp);
        assert_eq!(fixes[2].provider, crate::types::FixProvider::Nlp);
    }

    #[test]
    fn fixes_returns_empty_for_no_fixes() {
        let input = "\
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3";

        let records = parse_lines(input);
        let fixes: Vec<_> = records.into_iter().fixes().collect();
        assert!(fixes.is_empty());
    }

    #[test]
    fn primary_fixes_excludes_nlp() {
        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,9.9,,1771641930000,,,2092092474651730,2.8,0,,,
Fix,NLP,36.500,140.500,,,400.0,,1771641935000,,,,,,,,
Fix,FLP,36.213,140.098,285.0,1.0,5.0,,1771641940000,,,2092092474651730,2.8,0,,,";

        let records = parse_lines(input);
        let fixes: Vec<_> = records.into_iter().primary_fixes().collect();

        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0].provider, crate::types::FixProvider::Gps);
        assert_eq!(fixes[1].provider, crate::types::FixProvider::Flp);
    }

    #[test]
    fn primary_fixes_returns_empty_for_nlp_only() {
        let input = "\
Fix,NLP,36.500,140.500,,,400.0,,1771641935000,,,,,,,,
Fix,NLP,36.501,140.501,,,400.0,,1771641936000,,,,,,,,";

        let records = parse_lines(input);
        let fixes: Vec<_> = records.into_iter().primary_fixes().collect();
        assert!(fixes.is_empty());
    }
}
