use tokio::sync::Semaphore;

use crate::Algorithm;

use std::{cmp::Ordering, sync::Arc, time::Duration};

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
                self.semaphore
                    .add_permits(new_max_permits - self.max_permits);
                self.max_permits = new_max_permits;
            }
            Ordering::Less => {
                let excess_permits = self.max_permits - new_max_permits;
                let forgotten_permits = self.semaphore.forget_permits(excess_permits);
                self.max_permits -= forgotten_permits;
            }
            Ordering::Equal => {}
        }
    }
}
