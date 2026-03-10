use std::collections::VecDeque;
use std::time::Duration;

use tower_acc::Algorithm;

use super::engine::{RequestId, SimTime};

/// A queued request waiting to be processed.
struct QueuedRequest {
    request_id: RequestId,
    arrival_time: SimTime,
}

/// Result of trying to accept a request.
pub enum AcceptResult {
    /// Request started processing immediately (inflight was below limit).
    Processing,
    /// Request was enqueued (inflight at limit, queue had space).
    Queued,
    /// Request was rejected (inflight at limit, queue full).
    Rejected,
}

/// Model of a server with ACC-based admission control and a bounded queue.
///
/// Flow: request arrives → if inflight < ACC limit, process immediately.
/// Otherwise, if queue not full, enqueue. Otherwise, load shed.
pub struct ServerModel {
    algorithm: Box<dyn Algorithm>,
    queue_capacity: usize,
    inflight: usize,
    queue: VecDeque<QueuedRequest>,
}

impl ServerModel {
    pub fn new(algorithm: Box<dyn Algorithm>, queue_capacity: usize) -> Self {
        Self {
            algorithm,
            queue_capacity,
            inflight: 0,
            queue: VecDeque::new(),
        }
    }

    /// Try to accept a request. Returns the outcome so the caller knows
    /// whether to schedule a completion event.
    pub fn try_accept(&mut self, request_id: RequestId, arrival_time: SimTime) -> AcceptResult {
        if self.inflight < self.algorithm.max_concurrency() {
            self.inflight += 1;
            return AcceptResult::Processing;
        }
        if self.queue.len() < self.queue_capacity {
            self.queue.push_back(QueuedRequest {
                request_id,
                arrival_time,
            });
            return AcceptResult::Queued;
        }
        AcceptResult::Rejected
    }

    /// Called when a request finishes processing. Updates the ACC algorithm
    /// and tries to start the next queued request.
    /// Returns Some((request_id, arrival_time)) if a queued request was started.
    pub fn on_complete(&mut self, rtt: Duration, is_error: bool) -> Option<(RequestId, SimTime)> {
        self.inflight -= 1;
        self.algorithm.update(rtt, self.inflight, is_error, false);

        // Try to dequeue and start next request.
        if self.inflight < self.algorithm.max_concurrency() {
            if let Some(req) = self.queue.pop_front() {
                self.inflight += 1;
                return Some((req.request_id, req.arrival_time));
            }
        }
        None
    }

    pub fn queue_capacity(&self) -> usize {
        self.queue_capacity
    }

    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    pub fn inflight(&self) -> usize {
        self.inflight
    }

    pub fn limit(&self) -> usize {
        self.algorithm.max_concurrency()
    }
}
