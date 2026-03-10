use serde::Serialize;

use super::client::ClientModel;
use super::engine::SimTime;
use super::server::ServerModel;

#[derive(Debug, Clone, Serialize)]
pub struct TracePoint {
    /// Simulation time in seconds.
    pub time_secs: f64,
    /// Server ACC concurrency limit.
    pub server_limit: usize,
    /// Number of requests being processed by server.
    pub server_inflight: usize,
    /// Current server queue depth.
    pub server_queue_depth: usize,
    /// Client ACC concurrency limit.
    pub client_limit: usize,
    /// Number of requests in flight from client's perspective.
    pub client_inflight: usize,
    /// EMA of observed latencies in ms.
    pub observed_latency_ms: f64,
    /// Number of shed requests since last sample.
    pub shed_count: u64,
    /// Cumulative shed count.
    pub cumulative_shed: u64,
    /// Current regime baseline latency in ms.
    pub processing_latency_ms: f64,
    /// Theoretical concurrency from Little's Law: request_rate × mean_latency.
    pub theoretical_concurrency: f64,
    /// Server queue capacity (constant, for chart reference).
    pub queue_capacity: usize,
}

pub struct TraceSampler {
    trace: Vec<TracePoint>,
    latency_ema: f64,
    shed_since_last: u64,
    cumulative_shed: u64,
}

impl TraceSampler {
    pub fn new() -> Self {
        Self {
            trace: Vec::new(),
            latency_ema: 0.0,
            shed_since_last: 0,
            cumulative_shed: 0,
        }
    }

    pub fn record_latency(&mut self, latency_ms: f64) {
        const ALPHA: f64 = 0.1;
        if self.latency_ema == 0.0 {
            self.latency_ema = latency_ms;
        } else {
            self.latency_ema = ALPHA * latency_ms + (1.0 - ALPHA) * self.latency_ema;
        }
    }

    pub fn record_shed(&mut self) {
        self.shed_since_last += 1;
        self.cumulative_shed += 1;
    }

    pub fn sample(
        &mut self,
        now: SimTime,
        server: &ServerModel,
        client: &ClientModel,
        regime_baseline_ms: f64,
        request_rate: f64,
    ) {
        self.trace.push(TracePoint {
            time_secs: now.as_secs_f64(),
            server_limit: server.limit(),
            server_inflight: server.inflight(),
            server_queue_depth: server.queue_depth(),
            client_limit: client.limit(),
            client_inflight: client.inflight(),
            observed_latency_ms: self.latency_ema,
            shed_count: self.shed_since_last,
            cumulative_shed: self.cumulative_shed,
            processing_latency_ms: regime_baseline_ms,
            theoretical_concurrency: request_rate * regime_baseline_ms / 1000.0,
            queue_capacity: server.queue_capacity(),
        });
        self.shed_since_last = 0;
    }

    pub fn into_trace(self) -> Vec<TracePoint> {
        self.trace
    }
}
