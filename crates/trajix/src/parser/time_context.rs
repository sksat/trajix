use crate::error::ParseError;
use crate::parser::line::Record;
use crate::record::status::StatusRecord;

/// Single-record timestamp tracker for Status timestamp inference.
///
/// Tracks the most recent timestamp from any record and fills in missing
/// Status timestamps. Use this when processing records one-by-one
/// (e.g., in a streaming parser without iterators).
///
/// # Example
/// ```
/// use trajix::parser::time_context::TimestampInferer;
/// use trajix::parser::line::Record;
///
/// let mut inferer = TimestampInferer::new();
/// // inferer.annotate(&mut record) tracks timestamp and fills Status gaps
/// ```
pub struct TimestampInferer {
    last_timestamp_ms: Option<i64>,
}

impl TimestampInferer {
    /// Create a new timestamp inferer.
    pub fn new() -> Self {
        Self {
            last_timestamp_ms: None,
        }
    }

    /// Track a record's timestamp. Returns the timestamp if present.
    pub fn track(&mut self, record: &Record) -> Option<i64> {
        let ts = record.timestamp_ms();
        if let Some(t) = ts {
            self.last_timestamp_ms = Some(t);
        }
        ts
    }

    /// Fill missing Status timestamp from tracked state.
    pub fn infer_status(&self, status: &mut StatusRecord) {
        if status.unix_time_ms.is_none() {
            status.unix_time_ms = self.last_timestamp_ms;
        }
    }

    /// Track timestamp and infer missing Status timestamp in one call.
    pub fn annotate(&mut self, record: &mut Record) {
        // Track timestamp from any record
        if let Some(ts) = record.timestamp_ms() {
            self.last_timestamp_ms = Some(ts);
        }
        // Fill missing Status timestamps
        if let Record::Status(s) = record
            && s.unix_time_ms.is_none()
        {
            s.unix_time_ms = self.last_timestamp_ms;
        }
    }

    /// The most recently observed timestamp (milliseconds).
    pub fn last_timestamp_ms(&self) -> Option<i64> {
        self.last_timestamp_ms
    }
}

impl Default for TimestampInferer {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator adapter that infers timestamps for Status records.
///
/// Status records in GNSS Logger data sometimes have empty `UnixTimeMillis`.
/// This adapter fills in missing timestamps using the most recent timestamp
/// from a preceding record (backward-looking inference).
///
/// Records that already have timestamps are passed through unchanged.
pub struct TimeAnnotator<I> {
    inner: I,
    inferer: TimestampInferer,
}

impl<I> TimeAnnotator<I> {
    pub fn new(inner: I) -> Self {
        TimeAnnotator {
            inner,
            inferer: TimestampInferer::new(),
        }
    }

    /// The most recently observed timestamp (milliseconds).
    pub fn last_timestamp_ms(&self) -> Option<i64> {
        self.inferer.last_timestamp_ms()
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
            Ok(mut record) => {
                self.inferer.annotate(&mut record);
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
        text.lines().filter_map(parse_line).collect()
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
                assert!(
                    s.unix_time_ms.is_none(),
                    "no preceding record to infer from"
                );
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

    // ── TimestampInferer tests ──

    #[test]
    fn inferer_tracks_fix_timestamp() {
        let mut inferer = TimestampInferer::new();
        assert!(inferer.last_timestamp_ms().is_none());

        let records = parse_lines(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        let record = records[0].as_ref().unwrap();
        let ts = inferer.track(record);
        assert_eq!(ts, Some(1771641748000));
        assert_eq!(inferer.last_timestamp_ms(), Some(1771641748000));
    }

    #[test]
    fn inferer_infers_status() {
        let mut inferer = TimestampInferer::new();
        // Track a Fix timestamp
        let records = parse_lines(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        inferer.track(records[0].as_ref().unwrap());

        // Status with no timestamp
        let status_records =
            parse_lines("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");
        if let Ok(Record::Status(mut s)) = status_records.into_iter().next().unwrap() {
            assert!(s.unix_time_ms.is_none());
            inferer.infer_status(&mut s);
            assert_eq!(s.unix_time_ms, Some(1771641748000));
        } else {
            panic!("expected Status");
        }
    }

    #[test]
    fn inferer_preserves_existing_status() {
        let mut inferer = TimestampInferer::new();
        let records = parse_lines(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        inferer.track(records[0].as_ref().unwrap());

        let status_records = parse_lines(
            "Status,1771641934000,37,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1",
        );
        if let Ok(Record::Status(mut s)) = status_records.into_iter().next().unwrap() {
            assert_eq!(s.unix_time_ms, Some(1771641934000));
            inferer.infer_status(&mut s);
            // Should NOT overwrite existing timestamp
            assert_eq!(s.unix_time_ms, Some(1771641934000));
        } else {
            panic!("expected Status");
        }
    }

    #[test]
    fn inferer_annotate_combined() {
        let mut inferer = TimestampInferer::new();

        // Feed a Fix (track timestamp)
        let mut fix = parse_lines(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        )
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
        inferer.annotate(&mut fix);
        assert_eq!(inferer.last_timestamp_ms(), Some(1771641748000));

        // Feed a Status with no timestamp (should be inferred)
        let mut status =
            parse_lines("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1")
                .into_iter()
                .next()
                .unwrap()
                .unwrap();
        inferer.annotate(&mut status);
        if let Record::Status(s) = &status {
            assert_eq!(s.unix_time_ms, Some(1771641748000));
        } else {
            panic!("expected Status");
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
