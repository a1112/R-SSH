use std::cell::Cell;

thread_local! {
    static ACTIVE_CANDIDATE_BYTES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Exact sum of candidate-slice lengths passed to legacy byte-search and
/// prefix-match primitives while a terminal-output filter scan is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QueryScanWork {
    pub(crate) candidate_bytes: u64,
}

pub(crate) fn measure_query_scan_work<T>(scan: impl FnOnce() -> T) -> (T, QueryScanWork) {
    ACTIVE_CANDIDATE_BYTES.with(|active| {
        assert!(
            active.replace(Some(0)).is_none(),
            "query scan work measurement must not be nested"
        );
    });
    let mut guard = QueryScanGuard { completed: false };
    let result = scan();
    let candidate_bytes = ACTIVE_CANDIDATE_BYTES.with(|active| {
        active
            .replace(None)
            .expect("query scan work measurement must remain active")
    });
    guard.completed = true;
    (result, QueryScanWork { candidate_bytes })
}

pub(crate) fn record_query_scan_candidate(candidate: &[u8]) {
    let bytes = u64::try_from(candidate.len()).unwrap_or(u64::MAX);
    record_query_scan_candidate_bytes(bytes);
}

fn record_query_scan_candidate_bytes(bytes: u64) {
    ACTIVE_CANDIDATE_BYTES.with(|active| {
        if let Some(current) = active.get() {
            active.set(Some(current.saturating_add(bytes)));
        }
    });
}

struct QueryScanGuard {
    completed: bool,
}

impl Drop for QueryScanGuard {
    fn drop(&mut self) {
        if !self.completed {
            ACTIVE_CANDIDATE_BYTES.with(|active| active.set(None));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn measurement_counts_candidates_and_saturates_additions() {
        let ((), work) = super::measure_query_scan_work(|| {
            super::record_query_scan_candidate(b"abc");
            super::record_query_scan_candidate(b"defgh");
            super::record_query_scan_candidate_bytes(u64::MAX);
        });

        assert_eq!(work.candidate_bytes, u64::MAX);
    }
}
