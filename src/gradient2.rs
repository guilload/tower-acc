use std::time::Duration;

use crate::Algorithm;

/// Netflix Gradient2–inspired adaptive concurrency limit strategy.
///
/// Instead of estimating absolute queue depth (like Vegas), Gradient2 uses the
/// *gradient* (ratio) of long-term RTT to short-term RTT to detect queueing.
/// A configurable tolerance allows small RTT increases without triggering
/// limit reduction, making it more robust to natural latency variance.
///
/// The algorithm maintains two RTT estimates:
/// - **long RTT**: exponentially smoothed baseline (adapts slowly)
/// - **short RTT**: most recent observed RTT (reacts immediately)
///
/// Each update computes:
/// ```text
/// gradient  = clamp(tolerance × long_rtt / short_rtt, 0.5, 1.0)
/// new_limit = gradient × current_limit + queue_size
/// limit     = smooth(current_limit, new_limit)
/// ```
#[derive(Debug, Clone)]
pub struct Gradient2 {
    estimated_limit: f64,
    min_limit: usize,
    max_limit: usize,
    smoothing: f64,
    rtt_tolerance: f64,
    queue_size: fn(usize) -> usize,

    // Long-term RTT (exponentially smoothed).
    long_rtt_ns: f64,
    long_rtt_count: usize,
    long_rtt_warmup: usize,
    long_rtt_warmup_sum: f64,
    long_rtt_factor: f64,

    // Short-term RTT (last observed).
    last_rtt_ns: f64,
}

impl Gradient2 {
    /// Returns a [`Gradient2Builder`] for configuring a new `Gradient2` instance.
    pub fn builder() -> Gradient2Builder {
        Gradient2Builder::default()
    }
}

impl Default for Gradient2 {
    fn default() -> Self {
        Gradient2Builder::default().build()
    }
}

impl Algorithm for Gradient2 {
    fn max_concurrency(&self) -> usize {
        (self.estimated_limit as usize).clamp(self.min_limit, self.max_limit).max(1)
    }

    fn update(&mut self, rtt: Duration, num_inflight: usize, _is_error: bool, is_canceled: bool) {
        if is_canceled {
            return;
        }

        let rtt_ns = rtt.as_nanos() as f64;
        if rtt_ns <= 0.0 {
            return;
        }

        let limit = self.estimated_limit as usize;

        // Update short-term RTT.
        self.last_rtt_ns = rtt_ns;

        // Update long-term RTT (exponential average with warmup).
        if self.long_rtt_count < self.long_rtt_warmup {
            self.long_rtt_warmup_sum += rtt_ns;
            self.long_rtt_count += 1;
            self.long_rtt_ns = self.long_rtt_warmup_sum / self.long_rtt_count as f64;
        } else {
            self.long_rtt_ns =
                self.long_rtt_ns * (1.0 - self.long_rtt_factor) + rtt_ns * self.long_rtt_factor;
        }

        // If long RTT has drifted far above short RTT, pull it down to accelerate
        // recovery after sustained load.
        if self.long_rtt_ns / self.last_rtt_ns > 2.0 {
            self.long_rtt_ns *= 0.95;
        }

        // Don't adjust when system is lightly loaded — signal is unreliable.
        if num_inflight * 2 < limit {
            return;
        }

        // Compute gradient: ratio of baseline to current RTT, with tolerance.
        // gradient = 1.0 means latencies are stable; < 1.0 means queueing detected.
        let gradient =
            (self.rtt_tolerance * self.long_rtt_ns / self.last_rtt_ns).clamp(0.5, 1.0);

        let queue_size = (self.queue_size)(limit);
        let new_limit = gradient * self.estimated_limit + queue_size as f64;

        // Apply smoothing, then clamp to bounds.
        self.estimated_limit =
            ((1.0 - self.smoothing) * self.estimated_limit + self.smoothing * new_limit)
                .clamp(self.min_limit as f64, self.max_limit as f64);
    }
}

fn log10_queue_size(limit: usize) -> usize {
    std::cmp::max(1, (limit as f64).log10().ceil() as usize)
}

/// Builder for configuring a [`Gradient2`] algorithm instance.
///
/// See [`Gradient2::builder`] for usage.
///
/// # Defaults
///
/// | Parameter | Default |
/// |-----------|---------|
/// | `initial_limit` | 20 |
/// | `min_limit` | 20 |
/// | `max_limit` | 200 |
/// | `smoothing` | 0.2 |
/// | `rtt_tolerance` | 1.5 |
/// | `long_window` | 600 |
/// | `queue_size` | `ceil(log10(limit))` |
pub struct Gradient2Builder {
    initial_limit: usize,
    min_limit: usize,
    max_limit: usize,
    smoothing: f64,
    rtt_tolerance: f64,
    long_window: usize,
    queue_size: fn(usize) -> usize,
}

impl Default for Gradient2Builder {
    fn default() -> Self {
        Self {
            initial_limit: 20,
            min_limit: 20,
            max_limit: 200,
            smoothing: 0.2,
            rtt_tolerance: 1.5,
            long_window: 600,
            queue_size: log10_queue_size,
        }
    }
}

impl Gradient2Builder {
    /// Sets the starting concurrency limit (default: 20).
    pub fn initial_limit(mut self, limit: usize) -> Self {
        self.initial_limit = limit;
        self
    }

    /// Sets the minimum concurrency limit (default: 20).
    pub fn min_limit(mut self, limit: usize) -> Self {
        self.min_limit = limit;
        self
    }

    /// Sets the maximum concurrency limit (default: 200).
    pub fn max_limit(mut self, limit: usize) -> Self {
        self.max_limit = limit;
        self
    }

    /// Sets the smoothing factor for limit updates (default: 0.2).
    ///
    /// Lower values make the limit more stable but slower to react.
    /// Higher values make the limit more responsive but noisier.
    pub fn smoothing(mut self, smoothing: f64) -> Self {
        self.smoothing = smoothing;
        self
    }

    /// Sets the RTT tolerance ratio (default: 1.5).
    ///
    /// Values > 1.0 allow some RTT increase without reducing the limit.
    /// For example, 1.5 means RTT can increase 50% above baseline before
    /// the algorithm starts reducing concurrency.
    ///
    /// # Panics
    ///
    /// Panics if `tolerance` is less than 1.0.
    pub fn rtt_tolerance(mut self, tolerance: f64) -> Self {
        assert!(tolerance >= 1.0, "rtt_tolerance must be >= 1.0");
        self.rtt_tolerance = tolerance;
        self
    }

    /// Sets the window size for the long-term RTT exponential average
    /// (default: 600 samples). Larger values make the baseline more stable.
    pub fn long_window(mut self, window: usize) -> Self {
        self.long_window = window;
        self
    }

    /// Sets a function that computes the queue size (growth allowance)
    /// from the current limit (default: `ceil(log10(limit))`).
    pub fn queue_size(mut self, f: fn(usize) -> usize) -> Self {
        self.queue_size = f;
        self
    }

    /// Builds the [`Gradient2`] algorithm with the configured parameters.
    ///
    /// # Panics
    ///
    /// Panics if `min_limit` exceeds `max_limit`.
    pub fn build(self) -> Gradient2 {
        assert!(
            self.min_limit <= self.max_limit,
            "min_limit ({}) must be <= max_limit ({})",
            self.min_limit,
            self.max_limit,
        );
        let long_window = std::cmp::max(1, self.long_window);
        Gradient2 {
            estimated_limit: self.initial_limit as f64,
            min_limit: self.min_limit,
            max_limit: self.max_limit,
            smoothing: self.smoothing,
            rtt_tolerance: self.rtt_tolerance,
            queue_size: self.queue_size,
            long_rtt_ns: 0.0,
            long_rtt_count: 0,
            long_rtt_warmup: 10,
            long_rtt_warmup_sum: 0.0,
            long_rtt_factor: 2.0 / (long_window as f64 + 1.0),
            last_rtt_ns: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_rtt_allows_growth() {
        let mut g2 = Gradient2::builder().initial_limit(20).build();

        // Warmup: feed 10 samples at stable RTT to initialize long_rtt.
        for _ in 0..10 {
            g2.update(Duration::from_millis(50), 20, false, false);
        }

        let limit_before = g2.max_concurrency();
        // Stable RTT → gradient ≈ 1.0 → limit grows by queue_size.
        for _ in 0..20 {
            g2.update(Duration::from_millis(50), 20, false, false);
        }
        assert!(g2.max_concurrency() >= limit_before);
    }

    #[test]
    fn high_rtt_reduces_limit() {
        let mut g2 = Gradient2::builder().initial_limit(100).build();

        // Warmup at low RTT.
        for _ in 0..10 {
            g2.update(Duration::from_millis(50), 100, false, false);
        }

        let limit_before = g2.max_concurrency();
        // RTT spikes → gradient < 1.0 → limit decreases.
        for _ in 0..20 {
            g2.update(Duration::from_millis(500), 100, false, false);
        }
        assert!(g2.max_concurrency() < limit_before);
    }

    #[test]
    fn limit_respects_max() {
        let mut g2 = Gradient2::builder()
            .initial_limit(200)
            .max_limit(200)
            .min_limit(1)
            .build();

        // Warmup at stable RTT.
        for _ in 0..10 {
            g2.update(Duration::from_millis(50), 200, false, false);
        }

        // Many stable updates — limit should never exceed max.
        for _ in 0..100 {
            g2.update(Duration::from_millis(50), 200, false, false);
            assert!(g2.max_concurrency() <= 200);
        }
    }

    #[test]
    fn canceled_requests_are_ignored() {
        let mut g2 = Gradient2::builder().initial_limit(20).build();
        g2.update(Duration::from_millis(50), 20, false, true);
        assert_eq!(g2.max_concurrency(), 20);
    }

    #[test]
    fn limit_stays_above_min() {
        let mut g2 = Gradient2::builder()
            .initial_limit(20)
            .min_limit(10)
            .build();

        // Warmup at low RTT.
        for _ in 0..10 {
            g2.update(Duration::from_millis(50), 20, false, false);
        }
        // Sustained high RTT to drive limit down.
        for _ in 0..200 {
            g2.update(Duration::from_millis(500), 20, false, false);
        }
        assert!(g2.max_concurrency() >= 10);
    }

    #[test]
    fn tolerance_allows_moderate_rtt_increase() {
        let mut g2 = Gradient2::builder()
            .initial_limit(50)
            .rtt_tolerance(2.0)
            .build();

        // Warmup.
        for _ in 0..10 {
            g2.update(Duration::from_millis(50), 50, false, false);
        }

        // 1.5x RTT increase should still allow growth with tolerance=2.0
        // because tolerance * long_rtt / short_rtt = 2.0 * 50 / 75 ≈ 1.33 → clamped to 1.0.
        let limit_before = g2.max_concurrency();
        for _ in 0..10 {
            g2.update(Duration::from_millis(75), 50, false, false);
        }
        assert!(g2.max_concurrency() >= limit_before);
    }
}
