use crate::error::ParseError;
use crate::parser::line::Record;

/// Iterator adapter that infers timestamps for Status records.
///
/// Status records in GNSS Logger data sometimes have empty `UnixTimeMillis`.
/// This adapter fills in missing timestamps using the most recent timestamp
/// from a preceding record (backward-looking inference).
///
/// Records that already have timestamps are passed through unchanged.
pub struct TimeAnnotator<I> {
    inner: I,
    last_timestamp_ms: Option<i64>,
}

impl<I> TimeAnnotator<I> {
    pub fn new(inner: I) -> Self {
        TimeAnnotator {
            inner,
            last_timestamp_ms: None,
        }
    }

    /// The most recently observed timestamp (milliseconds).
    pub fn last_timestamp_ms(&self) -> Option<i64> {
        self.last_timestamp_ms
    }
}

impl<I> Iterator for TimeAnnotator<I>
where
    I: Iterator<Item = Result<Record, ParseError>>,
{
    type Item = Result<Record, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        match item {
            Ok(record) => {
                // Update tracked timestamp from any record that has one
                if let Some(ts) = record.timestamp_ms() {
                    self.last_timestamp_ms = Some(ts);
                }

                // Fill in missing Status timestamps
                let record = match record {
                    Record::Status(mut s) if s.unix_time_ms.is_none() => {
                        s.unix_time_ms = self.last_timestamp_ms;
                        Record::Status(s)
                    }
                    other => other,
                };

                Some(Ok(record))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Extension trait to add time annotation to any record iterator.
pub trait TimeAnnotate: Iterator<Item = Result<Record, ParseError>> + Sized {
    fn annotate_time(self) -> TimeAnnotator<Self> {
        TimeAnnotator::new(self)
    }
}

impl<I> TimeAnnotate for I where I: Iterator<Item = Result<Record, ParseError>> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::line::parse_line;

    /// Parse lines into records, filtering out blanks/comments.
    fn parse_lines(text: &str) -> Vec<Result<Record, ParseError>> {
        text.lines()
            .filter_map(|line| parse_line(line))
            .collect()
    }

    #[test]
    fn annotates_status_with_preceding_timestamp() {
        // Raw record (has timestamp), then Status records (empty timestamp)
        let input = "\
Raw,1771641934414,2610023102000000,18,0.0,-1453067129312867492,0.26299190521240234,36.5174091712106,-10.114003962039996,13.431297832655405,1778,2,0.0,16399,528352340926623,35,26.1,-622.2412719726562,0.7534999847412109,16,0.0,0.0,1575420030,,,,0,0.0,1,-53.45,22.5,0.0,0.0,,,C,2092091889651366,,-17071361.70072936,19492678.357181847,-3855303.537237491,-619.0749309725472,136.99272610661174,3167.5903074467838,12844.466062487014,0.0010315576000721194,0.000000016763806343078613,0.0,-0.00000011920928955078125,0.000000059604644775390625,122880.0,-32768.0,-196608.0,-65536.0
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Status,,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 3);

        // Raw record has its own timestamp
        assert_eq!(annotated[0].timestamp_ms(), Some(1771641934414));

        // Status records should have inferred timestamp from the Raw record
        match &annotated[1] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641934414));
            }
            _ => panic!("expected Status"),
        }
        match &annotated[2] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641934414));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn preserves_existing_status_timestamp() {
        // Status record with existing timestamp should be preserved
        let input = "Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 1);
        match &annotated[0] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641934000));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn status_at_start_gets_none_without_preceding() {
        // Status records at the very beginning with no preceding timestamp
        let input = "Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 1);
        match &annotated[0] {
            Record::Status(s) => {
                assert!(s.unix_time_ms.is_none(), "no preceding record to infer from");
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn timestamp_updates_through_mixed_records() {
        // Sensor → Status (gets sensor ts) → Fix → Status (gets fix ts)
        let input = "\
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Fix,GPS,36.2122343400,140.0985108900,284.0,0.91,9.9,112.1,1771641936000,0.82,25.9,2092092474651730,2.8,0,,,
Status,,46,0,5,26,1575420030,24.40,318.34833,81.3705,1,1,1,17.4";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 4);

        // First Status gets UncalAccel's timestamp
        match &annotated[1] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641935895));
            }
            _ => panic!("expected Status"),
        }

        // Second Status gets Fix's timestamp (more recent)
        match &annotated[3] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641936000));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn existing_status_timestamp_updates_tracker() {
        // Status with timestamp → Status without timestamp (should get the first's timestamp)
        let input = "\
Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Status,,37,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 2);

        // First Status keeps its timestamp
        match &annotated[0] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641934000));
            }
            _ => panic!("expected Status"),
        }

        // Second Status gets first's timestamp
        match &annotated[1] {
            Record::Status(s) => {
                assert_eq!(s.unix_time_ms, Some(1771641934000));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn non_status_records_pass_through_unchanged() {
        let input = "\
Fix,GPS,36.2122343400,140.0985108900,284.0,0.91,9.9,112.1,1771641936000,0.82,25.9,2092092474651730,2.8,0,,,
UncalAccel,1771641935895,2092092486937784,-3.3762727,-5.9318075,-2.469393,0.0,0.0,0.0,3";

        let records = parse_lines(input);
        let annotated: Vec<_> = records
            .into_iter()
            .annotate_time()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(annotated.len(), 2);
        assert_eq!(annotated[0].timestamp_ms(), Some(1771641936000));
        assert_eq!(annotated[1].timestamp_ms(), Some(1771641935895));
    }
}
