use std::time::Duration;

use crate::Algorithm;

fn log10(limit: usize) -> usize {
    std::cmp::max(1, (limit as f64).log10() as usize)
}

/// TCP Vegas–inspired adaptive concurrency limit strategy.
///
/// Estimates queue depth from the ratio of updated RTT to the minimum
/// (no-load) RTT, then adjusts the concurrency limit based on where the
/// estimated queue falls relative to configurable alpha/beta thresholds.
#[derive(Debug, Clone)]
pub struct Vegas {
    estimated_limit: f64,
    max_limit: usize,
    rtt_noload: Option<Duration>,
    smoothing: f64,
    alpha_fn: fn(usize) -> usize,
    beta_fn: fn(usize) -> usize,
    threshold_fn: fn(usize) -> usize,
    increase_fn: fn(f64) -> f64,
    decrease_fn: fn(f64) -> f64,
    probe_multiplier: usize,
    probe_count: usize,
    probe_jitter: f64,
}

impl Vegas {
    pub fn builder() -> VegasBuilder {
        VegasBuilder::default()
    }

    fn should_probe(&self, limit: usize) -> bool {
        let interval = (self.probe_jitter * self.probe_multiplier as f64 * limit as f64) as usize;
        interval > 0 && self.probe_count >= interval
    }

    /// Returns a random jitter in [0.5, 1.0) using a simple xorshift on the
    /// current probe count and estimated limit to avoid pulling in a RNG crate.
    fn next_jitter(&self) -> f64 {
        // Mix bits from the current state to produce a pseudo-random value.
        let mut x = (self.probe_count as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(self.estimated_limit.to_bits());
        x ^= x >> 30;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        // Map to [0.5, 1.0)
        0.5 + (x >> 11) as f64 / (1u64 << 53) as f64 * 0.5
    }
}

impl Default for Vegas {
    fn default() -> Self {
        VegasBuilder::default().build()
    }
}

impl Algorithm for Vegas {
    fn max_concurrency(&self) -> usize {
        std::cmp::max(1, self.estimated_limit as usize)
    }

    fn update(&mut self, rtt: Duration, num_inflight: usize, is_error: bool, is_canceled: bool) {
        if is_canceled {
            return;
        }

        self.probe_count += 1;

        let limit = self.estimated_limit as usize;

        // Periodically reset rtt_noload to track baseline changes.
        if self.should_probe(limit) {
            self.probe_count = 0;
            self.probe_jitter = self.next_jitter();
            self.rtt_noload = Some(rtt);
            return;
        }

        // Update rtt_noload, recording baseline on the first sample.
        let rtt_noload = match self.rtt_noload {
            Some(current) if rtt < current => {
                self.rtt_noload = Some(rtt);
                return;
            }
            Some(current) => current,
            None => {
                self.rtt_noload = Some(rtt);
                return;
            }
        };

        // Don't adjust the limit when the system is lightly loaded — low RTT
        // is a misleading signal when few requests are in-flight.
        if num_inflight * 2 < limit {
            return;
        }

        // Estimate queue depth: limit × (1 − rtt_noload / rtt).
        let rtt_nanos = rtt.as_nanos() as f64;
        let rtt_noload_nanos = rtt_noload.as_nanos() as f64;
        let queue_size =
            (self.estimated_limit * (1.0 - rtt_noload_nanos / rtt_nanos)).ceil() as usize;

        let alpha = (self.alpha_fn)(limit);
        let beta = (self.beta_fn)(limit);
        let threshold = (self.threshold_fn)(limit);

        let new_limit = if is_error {
            // Errors (timeouts / overload) immediately decrease.
            (self.decrease_fn)(self.estimated_limit)
        } else if queue_size <= threshold {
            // Very short queue — aggressive increase.
            self.estimated_limit + beta as f64
        } else if queue_size < alpha {
            // Short queue — gradual increase.
            (self.increase_fn)(self.estimated_limit)
        } else if queue_size > beta {
            // Long queue — decrease.
            (self.decrease_fn)(self.estimated_limit)
        } else {
            // Within [alpha, beta] — no change.
            return;
        };

        let new_limit = new_limit.clamp(1.0, self.max_limit as f64);
        self.estimated_limit =
            (1.0 - self.smoothing) * self.estimated_limit + self.smoothing * new_limit;
    }
}

pub struct VegasBuilder {
    initial_limit: usize,
    max_limit: usize,
    smoothing: f64,
    alpha_fn: fn(usize) -> usize,
    beta_fn: fn(usize) -> usize,
    threshold_fn: fn(usize) -> usize,
    increase_fn: fn(f64) -> f64,
    decrease_fn: fn(f64) -> f64,
    probe_multiplier: usize,
}

impl Default for VegasBuilder {
    fn default() -> Self {
        Self {
            initial_limit: 20,
            max_limit: 1000,
            smoothing: 1.0,
            alpha_fn: |limit| 3 * log10(limit),
            beta_fn: |limit| 6 * log10(limit),
            threshold_fn: log10,
            increase_fn: |limit| limit + log10(limit as usize) as f64,
            decrease_fn: |limit| limit - log10(limit as usize) as f64,
            probe_multiplier: 30,
        }
    }
}

impl VegasBuilder {
    pub fn initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    pub fn max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    pub fn smoothing(mut self, smoothing: f64) -> Self {
        self.smoothing = smoothing;
        self
    }

    pub fn alpha(mut self, f: fn(usize) -> usize) -> Self {
        self.alpha_fn = f;
        self
    }

    pub fn beta(mut self, f: fn(usize) -> usize) -> Self {
        self.beta_fn = f;
        self
    }

    pub fn threshold(mut self, f: fn(usize) -> usize) -> Self {
        self.threshold_fn = f;
        self
    }

    pub fn increase(mut self, f: fn(f64) -> f64) -> Self {
        self.increase_fn = f;
        self
    }

    pub fn decrease(mut self, f: fn(f64) -> f64) -> Self {
        self.decrease_fn = f;
        self
    }

    pub fn probe_multiplier(mut self, multiplier: usize) -> Self {
        self.probe_multiplier = multiplier;
        self
    }

    pub fn build(self) -> Vegas {
        Vegas {
            estimated_limit: self.initial_limit as f64,
            max_limit: self.max_limit,
            rtt_noload: None,
            smoothing: self.smoothing,
            alpha_fn: self.alpha_fn,
            beta_fn: self.beta_fn,
            threshold_fn: self.threshold_fn,
            increase_fn: self.increase_fn,
            decrease_fn: self.decrease_fn,
            probe_multiplier: self.probe_multiplier,
            probe_count: 0,
            probe_jitter: 0.5 + (self.initial_limit as f64 / self.max_limit as f64) * 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increase_limit_on_low_queue() {
        let mut vegas = Vegas::builder().initial_limit(10).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        // RTT just slightly above baseline → small queue → should increase.
        vegas.update(Duration::from_millis(11), 10, false, false);
        assert!(vegas.max_concurrency() > 10);
    }

    #[test]
    fn decrease_limit_on_high_queue() {
        let mut vegas = Vegas::builder().initial_limit(10).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        // RTT far above baseline → large queue → should decrease.
        vegas.update(Duration::from_millis(50), 10, false, false);
        assert!(vegas.max_concurrency() < 10);
    }

    #[test]
    fn decrease_limit_on_error() {
        let mut vegas = Vegas::builder().initial_limit(10).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        vegas.update(Duration::from_millis(10), 10, true, false);
        assert!(vegas.max_concurrency() < 10);
    }

    #[test]
    fn no_change_within_thresholds() {
        let mut vegas = Vegas::builder().initial_limit(10).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        // RTT producing queue_size in [alpha, beta] range → no change.
        // alpha(10) = 3, beta(10) = 6. We need queue_size between 3 and 6.
        // queue = limit * (1 - noload/rtt) = 10 * (1 - 10/rtt)
        // For queue = 4: rtt = 10 / (1 - 4/10) = 10/0.6 ≈ 16.67ms
        vegas.update(Duration::from_nanos(16_670_000), 10, false, false);
        assert_eq!(vegas.max_concurrency(), 10);
    }

    #[test]
    fn canceled_requests_are_ignored() {
        let mut vegas = Vegas::builder().initial_limit(10).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        vegas.update(Duration::from_millis(50), 10, false, true);
        assert_eq!(vegas.max_concurrency(), 10);
    }

    #[test]
    fn smoothing_dampens_changes() {
        let mut vegas = Vegas::builder().initial_limit(100).smoothing(0.5).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        // Error → decrease. With smoothing 0.5, the change should be dampened.
        vegas.update(Duration::from_millis(10), 100, true, false);
        let limit = vegas.max_concurrency();
        // Without smoothing: 100 - log10(100) = 98
        // With smoothing 0.5: 0.5 * 100 + 0.5 * 98 = 99
        assert_eq!(limit, 99);
    }

    #[test]
    fn limit_never_below_one() {
        let mut vegas = Vegas::builder().initial_limit(1).build();
        vegas.rtt_noload = Some(Duration::from_millis(10));

        for _ in 0..100 {
            vegas.update(Duration::from_millis(10), 1, true, false);
        }
        assert_eq!(vegas.max_concurrency(), 1);
    }
}
