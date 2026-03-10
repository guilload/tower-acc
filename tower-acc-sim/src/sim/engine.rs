use std::cmp::Ordering;
use std::collections::BinaryHeap;

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::client::ClientModel;
use super::config::SimConfig;
use super::server::{AcceptResult, ServerModel};
use super::trace::{TracePoint, TraceSampler};

/// Simulated time in nanoseconds since simulation start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimTime(pub u64);

impl SimTime {
    pub fn from_secs_f64(secs: f64) -> Self {
        Self((secs * 1_000_000_000.0) as u64)
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }
}

/// A unique identifier for a request flowing through the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub u64);

#[derive(Debug)]
pub enum Event {
    /// Client generates a new request.
    RequestArrival,
    /// Server finishes processing a request.
    RequestComplete {
        /// When the client sent the request (for client RTT).
        arrival_time: SimTime,
        /// When the server started processing (for server RTT).
        processing_start: SimTime,
    },
    /// Server latency distribution changes.
    LatencyRegimeChange { regime_index: usize },
    /// Sample trace data.
    TraceSample,
}

/// An event scheduled at a specific simulation time.
struct ScheduledEvent {
    time: SimTime,
    event: Event,
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior.
        other.time.cmp(&self.time)
    }
}

pub struct EventQueue {
    heap: BinaryHeap<ScheduledEvent>,
}

impl EventQueue {
    fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    pub fn schedule(&mut self, time: SimTime, event: Event) {
        self.heap.push(ScheduledEvent { time, event });
    }

    fn pop(&mut self) -> Option<(SimTime, Event)> {
        self.heap.pop().map(|se| (se.time, se.event))
    }
}

/// Run the full simulation and return the trace.
pub fn run(config: &SimConfig) -> Vec<TracePoint> {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut queue = EventQueue::new();

    let mut server = ServerModel::new(config.server_algorithm.build(), config.queue_capacity);
    let mut client = ClientModel::new(config.client_algorithm.build());
    let mut sampler = TraceSampler::new();

    let end_time = SimTime::from_secs_f64(config.duration_secs);

    // Schedule latency regime changes.
    for (i, regime) in config.regimes.iter().enumerate() {
        queue.schedule(
            SimTime::from_secs_f64(regime.start_secs),
            Event::LatencyRegimeChange { regime_index: i },
        );
    }

    // Schedule first request arrival.
    let first_arrival = exponential_sample(&mut rng, config.request_rate);
    queue.schedule(SimTime::from_secs_f64(first_arrival), Event::RequestArrival);

    // Schedule first trace sample.
    queue.schedule(SimTime::from_secs_f64(0.05), Event::TraceSample);

    let mut current_regime: usize = 0;
    let mut next_request_id: u64 = 0;

    while let Some((now, event)) = queue.pop() {
        if now >= end_time {
            break;
        }

        match event {
            Event::RequestArrival => {
                let request_id = RequestId(next_request_id);
                next_request_id += 1;

                // Client-side throttling.
                if client.can_send() {
                    client.on_send();

                    // Server admission control.
                    match server.try_accept(request_id, now) {
                        AcceptResult::Processing => {
                            // Started immediately — schedule completion.
                            let regime = &config.regimes[current_regime];
                            let latency_ns = sample_lognormal_latency(
                                &mut rng,
                                regime.mean_latency_ms,
                                regime.std_factor,
                            );
                            let complete_time = SimTime(now.0 + latency_ns);
                            queue.schedule(
                                complete_time,
                                Event::RequestComplete {
                                    arrival_time: now,
                                    processing_start: now,
                                },
                            );
                        }
                        AcceptResult::Queued => {
                            // Sitting in queue — completion will be scheduled
                            // when a slot opens in on_complete().
                        }
                        AcceptResult::Rejected => {
                            // Load shed by server.
                            let rtt = std::time::Duration::from_millis(1);
                            client.on_response(rtt, true);
                            sampler.record_shed();
                        }
                    }
                }

                // Schedule next arrival.
                let inter_arrival = exponential_sample(&mut rng, config.request_rate);
                queue.schedule(
                    SimTime(now.0 + (inter_arrival * 1_000_000_000.0) as u64),
                    Event::RequestArrival,
                );
            }

            Event::RequestComplete { arrival_time, processing_start } => {
                // Server sees only processing time (like real tower-acc).
                let server_rtt = std::time::Duration::from_nanos(now.0 - processing_start.0);
                // Client sees full arrival-to-completion time.
                let client_rtt_ns = now.0 - arrival_time.0;
                let client_rtt = std::time::Duration::from_nanos(client_rtt_ns);

                // Update server ACC and try to dequeue.
                if let Some((_rid, queued_arrival)) = server.on_complete(server_rtt, false) {
                    // A queued request just started processing now.
                    let regime = &config.regimes[current_regime];
                    let latency_ns =
                        sample_lognormal_latency(&mut rng, regime.mean_latency_ms, regime.std_factor);
                    let complete_time = SimTime(now.0 + latency_ns);
                    queue.schedule(
                        complete_time,
                        Event::RequestComplete {
                            arrival_time: queued_arrival,
                            processing_start: now,
                        },
                    );
                }

                // Update client ACC.
                client.on_response(client_rtt, false);
                sampler.record_latency(client_rtt_ns as f64 / 1_000_000.0);
            }

            Event::LatencyRegimeChange { regime_index } => {
                current_regime = regime_index;
            }

            Event::TraceSample => {
                let regime = &config.regimes[current_regime];
                sampler.sample(now, &server, &client, regime.mean_latency_ms, config.request_rate);
                queue.schedule(SimTime(now.0 + 50_000_000), Event::TraceSample);
            }
        }
    }

    sampler.into_trace()
}

/// Sample from exponential distribution (inter-arrival time for Poisson process).
fn exponential_sample(rng: &mut StdRng, rate: f64) -> f64 {
    let u: f64 = rng.random();
    -u.ln() / rate
}

/// Sample latency from a log-normal distribution. Returns nanoseconds.
fn sample_lognormal_latency(rng: &mut StdRng, mean_ms: f64, sigma: f64) -> u64 {
    let mu = mean_ms.ln();
    // Box-Muller transform.
    let u1: f64 = rng.random();
    let u2: f64 = rng.random();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    let latency_ms = (mu + sigma * z).exp();
    (latency_ms * 1_000_000.0) as u64
}
