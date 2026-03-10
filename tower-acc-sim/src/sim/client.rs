use std::time::Duration;

use tower_acc::Algorithm;

/// Model of a client with ACC-based concurrency control.
pub struct ClientModel {
    algorithm: Box<dyn Algorithm>,
    inflight: usize,
}

impl ClientModel {
    pub fn new(algorithm: Box<dyn Algorithm>) -> Self {
        Self {
            algorithm,
            inflight: 0,
        }
    }

    /// Check if the client can send another request (inflight < limit).
    pub fn can_send(&self) -> bool {
        self.inflight < self.algorithm.max_concurrency()
    }

    /// Record that a request was sent.
    pub fn on_send(&mut self) {
        self.inflight += 1;
    }

    /// Record a response (success or error) and update the algorithm.
    pub fn on_response(&mut self, rtt: Duration, is_error: bool) {
        if self.inflight > 0 {
            self.inflight -= 1;
        }
        self.algorithm
            .update(rtt, self.inflight, is_error, false);
    }

    pub fn inflight(&self) -> usize {
        self.inflight
    }

    pub fn limit(&self) -> usize {
        self.algorithm.max_concurrency()
    }
}
