use std::time::Duration;

use crate::Algorithm;

/// AIMD (Additive Increase / Multiplicative Decrease) concurrency limit
/// strategy.
///
/// A loss-based algorithm that increases the limit by a fixed amount on
/// success and multiplies it by a backoff ratio on error or timeout.
/// This is the same approach used in TCP Reno congestion control.
///
/// Unlike [`Vegas`](crate::Vegas), AIMD does not track baseline RTT or
/// estimate queue depth — it reacts purely to errors and timeouts, making it
/// simpler but less proactive.
///
/// # Differences from Netflix's Java implementation
///
/// The Java reference truncates the limit to an integer after every update,
/// while this implementation keeps it as an `f64` internally. This means
/// repeated backoffs decay more smoothly (e.g. 10.0 → 9.0 → 8.1 → 7.29
/// instead of 10 → 9 → 8 → 7). The observable limit (via
/// [`max_concurrency`](Algorithm::max_concurrency)) is the same in most
/// cases, but after recovery the internal state may be slightly higher than
/// the Java equivalent, leading to marginally faster ramp-up.
#[derive(Debug, Clone)]
pub struct Aimd {
    estimated_limit: f64,
    min_limit: usize,
    max_limit: usize,
    backoff_ratio: f64,
    timeout: Duration,
}

impl Aimd {
    /// Returns an `AimdBuilder` for configuring a new `Aimd` instance.
    pub fn builder() -> AimdBuilder {
        AimdBuilder::default()
    }
}

impl Default for Aimd {
    fn default() -> Self {
        AimdBuilder::default().build()
    }
}

impl Algorithm for Aimd {
    fn max_concurrency(&self) -> usize {
        (self.estimated_limit as usize).clamp(self.min_limit, self.max_limit)
    }

    fn update(&mut self, rtt: Duration, num_inflight: usize, is_error: bool, is_canceled: bool) {
        if is_canceled {
            return;
        }

        let limit = self.estimated_limit;

        let new_limit = if is_error || rtt > self.timeout {
            // Multiplicative decrease.
            limit * self.backoff_ratio
        } else if num_inflight * 2 >= limit as usize {
            // Additive increase — only when the system is reasonably loaded.
            limit + 1.0
        } else {
            return;
        };

        self.estimated_limit = new_limit.clamp(self.min_limit as f64, self.max_limit as f64);
    }
}

/// Builder for configuring an [`Aimd`] algorithm instance.
///
/// See [`Aimd::builder`] for usage.
///
/// # Defaults
///
/// | Parameter | Default |
/// |-----------|---------|
/// | `initial_limit` | 20 |
/// | `min_limit` | 20 |
/// | `max_limit` | 200 |
/// | `backoff_ratio` | 0.9 |
/// | `timeout` | 5 seconds |
pub struct AimdBuilder {
    initial_limit: usize,
    min_limit: usize,
    max_limit: usize,
    backoff_ratio: f64,
    timeout: Duration,
}

impl Default for AimdBuilder {
    fn default() -> Self {
        Self {
            initial_limit: 20,
            min_limit: 20,
            max_limit: 200,
            backoff_ratio: 0.9,
            timeout: Duration::from_secs(5),
        }
    }
}

impl AimdBuilder {
    /// Sets the starting concurrency limit (default: 20).
    pub fn initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    /// Sets the lower bound the limit can reach (default: 20).
    pub fn min_limit(mut self, limit: usize) -> Self {
        self.min_limit = limit;
        self
    }

    /// Sets the upper bound the limit can reach (default: 200).
    pub fn max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    /// Sets the multiplicative backoff ratio applied on errors or timeouts
    /// (default: 0.9). Must be in `(0, 1)`.
    pub fn backoff_ratio(mut self, ratio: f64) -> Self {
        self.backoff_ratio = ratio;
        self
    }

    /// Sets the RTT threshold above which a request is treated as a timeout
    /// (default: 5 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Builds the [`Aimd`] algorithm with the configured parameters.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `backoff_ratio` is not in `[0.5, 1.0)`
    /// - `min_limit` is zero
    /// - `min_limit > max_limit`
    /// - `initial_limit < min_limit` or `initial_limit > max_limit`
    pub fn build(self) -> Aimd {
        assert!(
            (0.5..1.0).contains(&self.backoff_ratio),
            "backoff_ratio must be in [0.5, 1.0), got {}",
            self.backoff_ratio,
        );
        assert!(self.min_limit > 0, "min_limit must be > 0");
        assert!(
            self.min_limit <= self.max_limit,
            "min_limit ({}) must be <= max_limit ({})",
            self.min_limit,
            self.max_limit,
        );
        assert!(
            self.initial_limit >= self.min_limit && self.initial_limit <= self.max_limit,
            "initial_limit ({}) must be in [min_limit({}), max_limit({})]",
            self.initial_limit,
            self.min_limit,
            self.max_limit,
        );

        Aimd {
            estimated_limit: self.initial_limit as f64,
            min_limit: self.min_limit,
            max_limit: self.max_limit,
            backoff_ratio: self.backoff_ratio,
            timeout: self.timeout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increase_limit_on_success_when_loaded() {
        let mut aimd = Aimd::builder().initial_limit(10).min_limit(1).build();

        // Inflight * 2 >= limit → loaded → should increase by 1.
        aimd.update(Duration::from_millis(50), 10, false, false);
        assert_eq!(aimd.max_concurrency(), 11);
    }

    #[test]
    fn no_increase_when_lightly_loaded() {
        let mut aimd = Aimd::builder().initial_limit(10).min_limit(1).build();

        // Inflight * 2 < limit → not loaded enough → no change.
        aimd.update(Duration::from_millis(50), 2, false, false);
        assert_eq!(aimd.max_concurrency(), 10);
    }

    #[test]
    fn decrease_limit_on_error() {
        let mut aimd = Aimd::builder().initial_limit(10).min_limit(1).build();

        aimd.update(Duration::from_millis(50), 10, true, false);
        assert_eq!(aimd.max_concurrency(), 9); // 10 * 0.9 = 9
    }

    #[test]
    fn decrease_limit_on_timeout() {
        let mut aimd = Aimd::builder()
            .initial_limit(10)
            .min_limit(1)
            .timeout(Duration::from_secs(1))
            .build();

        // RTT exceeds timeout → treat as error.
        aimd.update(Duration::from_secs(2), 10, false, false);
        assert_eq!(aimd.max_concurrency(), 9);
    }

    #[test]
    fn canceled_requests_are_ignored() {
        let mut aimd = Aimd::builder().initial_limit(10).min_limit(1).build();

        aimd.update(Duration::from_millis(50), 10, true, true);
        assert_eq!(aimd.max_concurrency(), 10);
    }

    #[test]
    fn limit_does_not_drop_below_min() {
        let mut aimd = Aimd::builder()
            .initial_limit(5)
            .min_limit(5)
            .build();

        for _ in 0..100 {
            aimd.update(Duration::from_millis(50), 10, true, false);
        }
        assert_eq!(aimd.max_concurrency(), 5);
    }

    #[test]
    fn limit_does_not_exceed_max() {
        let mut aimd = Aimd::builder()
            .initial_limit(10)
            .min_limit(1)
            .max_limit(12)
            .build();

        for _ in 0..100 {
            aimd.update(Duration::from_millis(50), 10, false, false);
        }
        assert_eq!(aimd.max_concurrency(), 12);
    }

    #[test]
    fn custom_backoff_ratio() {
        let mut aimd = Aimd::builder()
            .initial_limit(100)
            .min_limit(1)
            .backoff_ratio(0.5)
            .build();

        aimd.update(Duration::from_millis(50), 100, true, false);
        assert_eq!(aimd.max_concurrency(), 50); // 100 * 0.5 = 50
    }

    #[test]
    #[should_panic(expected = "backoff_ratio must be in [0.5, 1.0)")]
    fn rejects_backoff_ratio_too_low() {
        Aimd::builder().backoff_ratio(0.3).build();
    }

    #[test]
    #[should_panic(expected = "backoff_ratio must be in [0.5, 1.0)")]
    fn rejects_backoff_ratio_ge_one() {
        Aimd::builder().backoff_ratio(1.0).build();
    }

    #[test]
    #[should_panic(expected = "min_limit must be > 0")]
    fn rejects_zero_min_limit() {
        Aimd::builder().min_limit(0).build();
    }

    #[test]
    #[should_panic(expected = "min_limit (50) must be <= max_limit (10)")]
    fn rejects_min_exceeds_max() {
        Aimd::builder().min_limit(50).max_limit(10).initial_limit(50).build();
    }

    #[test]
    #[should_panic(expected = "initial_limit (5) must be in")]
    fn rejects_initial_below_min() {
        Aimd::builder().initial_limit(5).min_limit(10).max_limit(100).build();
    }
}
