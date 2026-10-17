/// Runtime counters for a shared outbound HTTP client.
///
/// Plain data with no dependency on the client itself, so traits defined in
/// `sisyphus-types` can expose transport metrics without dragging the HTTP
/// implementation (and `reqwest`) into this crate (ADR-014 phase 3).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HttpClientStats {
    /// Total requests issued since start-up.
    pub requests_total: u64,
    /// Requests that failed (non-2xx or transport error).
    pub errors_total: u64,
    /// Requests issued but not yet answered.
    pub in_flight: u64,
    /// Completed graceful shutdowns.
    pub shutdowns_total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zero() {
        let s = HttpClientStats::default();
        assert_eq!(s.requests_total, 0);
        assert_eq!(s.errors_total, 0);
    }

    #[test]
    fn is_copy_and_comparable() {
        let a = HttpClientStats {
            requests_total: 3,
            errors_total: 1,
            in_flight: 2,
            shutdowns_total: 0,
        };
        let mut b = a;
        b.requests_total = 4;
        assert_ne!(a, b);
        assert_eq!(a.requests_total, 3, "copy must not alias");
    }
}
