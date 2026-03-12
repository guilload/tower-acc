use tokio::sync::Semaphore;

use crate::Algorithm;

use std::{cmp::Ordering, time::Duration};

use crate::sync::Arc;

/// Updates the algorithm after each request completes and resizes the semaphore to match the new concurrency limit.
pub(crate) struct Controller<A> {
    algorithm: A,
    semaphore: Arc<Semaphore>,
    max_permits: usize,
}

impl<A: Algorithm> Controller<A> {
    pub(crate) fn new(algorithm: A) -> Self {
        let max_permits = algorithm.max_concurrency();
        let semaphore = Arc::new(Semaphore::new(max_permits));

        Self {
            algorithm,
            semaphore,
            max_permits,
        }
    }

    pub(crate) fn semaphore(&self) -> Arc<Semaphore> {
        self.semaphore.clone()
    }

    /// Updates the algorithm with a completed request's outcome and resizes the
    /// semaphore to match the new concurrency limit.
    pub(crate) fn update(&mut self, rtt: Duration, is_error: bool, is_canceled: bool) {
        let num_inflight = self.max_permits - self.semaphore.available_permits();
        self.algorithm
            .update(rtt, num_inflight, is_error, is_canceled);
        self.resize();
    }

    fn resize(&mut self) {
        let new_max_permits = self.algorithm.max_concurrency();

        match new_max_permits.cmp(&self.max_permits) {
            Ordering::Greater => {
                #[cfg(feature = "tracing")]
                let previous_concurrency_limit = self.max_permits;

                self.semaphore
                    .add_permits(new_max_permits - self.max_permits);
                self.max_permits = new_max_permits;

                #[cfg(feature = "tracing")]
                tracing::info!(
                    gauge.concurrency_limit = self.max_permits,
                    previous_concurrency_limit
                );
            }
            Ordering::Less => {
                #[cfg(feature = "tracing")]
                let previous_concurrency_limit = self.max_permits;

                let excess_permits = self.max_permits - new_max_permits;
                let forgotten_permits = self.semaphore.forget_permits(excess_permits);
                self.max_permits -= forgotten_permits;

                #[cfg(feature = "tracing")]
                tracing::info!(
                    gauge.concurrency_limit = self.max_permits,
                    previous_concurrency_limit
                );
            }
            Ordering::Equal => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test algorithm with a directly settable limit.
    struct FakeAlgorithm {
        limit: usize,
    }

    impl FakeAlgorithm {
        fn new(limit: usize) -> Self {
            Self { limit }
        }
    }

    impl Algorithm for FakeAlgorithm {
        fn max_concurrency(&self) -> usize {
            self.limit
        }

        fn update(
            &mut self,
            _rtt: Duration,
            _num_inflight: usize,
            _is_error: bool,
            _is_canceled: bool,
        ) {
            // No-op — tests set `limit` directly.
        }
    }

    #[test]
    fn new_initializes_semaphore_to_algorithm_limit() {
        let controller = Controller::new(FakeAlgorithm::new(10));
        assert_eq!(controller.semaphore.available_permits(), 10);
        assert_eq!(controller.max_permits, 10);
    }

    #[test]
    fn resize_adds_permits_when_limit_increases() {
        let mut controller = Controller::new(FakeAlgorithm::new(10));
        assert_eq!(controller.semaphore.available_permits(), 10);

        controller.algorithm.limit = 15;
        controller.resize();

        assert_eq!(controller.semaphore.available_permits(), 15);
        assert_eq!(controller.max_permits, 15);
    }

    #[test]
    fn resize_forgets_permits_when_limit_decreases() {
        let mut controller = Controller::new(FakeAlgorithm::new(10));
        assert_eq!(controller.semaphore.available_permits(), 10);

        controller.algorithm.limit = 6;
        controller.resize();

        assert_eq!(controller.semaphore.available_permits(), 6);
        assert_eq!(controller.max_permits, 6);
    }

    #[test]
    fn resize_is_noop_when_limit_unchanged() {
        let mut controller = Controller::new(FakeAlgorithm::new(10));
        controller.resize();
        assert_eq!(controller.semaphore.available_permits(), 10);
        assert_eq!(controller.max_permits, 10);
    }

    #[test]
    fn update_passes_inflight_count_to_algorithm() {
        /// Algorithm that records the num_inflight it received.
        struct RecordingAlgorithm {
            limit: usize,
            last_inflight: Option<usize>,
        }

        impl Algorithm for RecordingAlgorithm {
            fn max_concurrency(&self) -> usize {
                self.limit
            }

            fn update(
                &mut self,
                _rtt: Duration,
                num_inflight: usize,
                _is_error: bool,
                _is_canceled: bool,
            ) {
                self.last_inflight = Some(num_inflight);
            }
        }

        let mut controller = Controller::new(RecordingAlgorithm {
            limit: 10,
            last_inflight: None,
        });

        // Acquire 3 permits to simulate 3 in-flight requests.
        let _p1 = controller.semaphore.clone().try_acquire_owned().unwrap();
        let _p2 = controller.semaphore.clone().try_acquire_owned().unwrap();
        let _p3 = controller.semaphore.clone().try_acquire_owned().unwrap();

        controller.update(Duration::from_millis(50), false, false);

        assert_eq!(controller.algorithm.last_inflight, Some(3));
    }

    #[test]
    fn resize_decrease_with_held_permits() {
        let mut controller = Controller::new(FakeAlgorithm::new(10));

        // Simulate 8 in-flight requests holding permits.
        let mut held = Vec::new();
        for _ in 0..8 {
            held.push(controller.semaphore.clone().try_acquire_owned().unwrap());
        }
        assert_eq!(controller.semaphore.available_permits(), 2);

        // Shrink limit to 6 — only 2 idle permits can be forgotten.
        controller.algorithm.limit = 6;
        controller.resize();

        // The 2 available permits were forgotten, so available is now 0.
        assert_eq!(controller.semaphore.available_permits(), 0);
        // max_permits decremented by the 2 we could actually forget.
        assert_eq!(controller.max_permits, 8);

        // As held permits are released, they become available again.
        drop(held.pop());
        assert_eq!(controller.semaphore.available_permits(), 1);
    }

    #[test]
    fn sequential_resize_up_then_down() {
        let mut controller = Controller::new(FakeAlgorithm::new(5));

        controller.algorithm.limit = 20;
        controller.resize();
        assert_eq!(controller.semaphore.available_permits(), 20);

        controller.algorithm.limit = 8;
        controller.resize();
        assert_eq!(controller.semaphore.available_permits(), 8);
        assert_eq!(controller.max_permits, 8);
    }
}
