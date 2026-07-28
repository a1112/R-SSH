use std::cell::Cell;

pub(crate) trait QueryScanRecorder: Copy {
    /// Records the complete candidate slice received by a concrete query
    /// search or prefix-match primitive.
    fn record_candidate(self, candidate: &[u8]);
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct QueryScanNoop;

impl QueryScanRecorder for QueryScanNoop {
    #[inline]
    fn record_candidate(self, _candidate: &[u8]) {}
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct QueryScanWork<'a> {
    candidate_bytes: &'a Cell<u64>,
}

impl<'a> QueryScanWork<'a> {
    pub(crate) const fn new(candidate_bytes: &'a Cell<u64>) -> Self {
        Self { candidate_bytes }
    }
}

impl QueryScanRecorder for QueryScanWork<'_> {
    #[inline]
    fn record_candidate(self, candidate: &[u8]) {
        let bytes = u64::try_from(candidate.len()).unwrap_or(u64::MAX);
        self.candidate_bytes
            .set(self.candidate_bytes.get().saturating_add(bytes));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{QueryScanNoop, QueryScanRecorder, QueryScanWork};

    fn record_two_candidates<R: QueryScanRecorder>(recorder: R) {
        recorder.record_candidate(b"abc");
        recorder.record_candidate(b"defgh");
    }

    #[test]
    fn work_recorder_counts_candidates_while_noop_has_no_state() {
        record_two_candidates(QueryScanNoop);

        let candidate_bytes = Cell::new(0);
        record_two_candidates(QueryScanWork::new(&candidate_bytes));

        assert_eq!(candidate_bytes.get(), 8);

        candidate_bytes.set(u64::MAX - 1);
        QueryScanWork::new(&candidate_bytes).record_candidate(b"abc");
        assert_eq!(candidate_bytes.get(), u64::MAX);
    }
}
