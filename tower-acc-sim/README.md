# tower-acc-sim

Interactive simulator for [tower-acc](../README.md)'s adaptive concurrency
control algorithms. Run discrete-event simulations in the browser and watch how
AIMD, Gradient2, and Vegas respond to changing server conditions in real time.

## Quick start

```sh
cargo run -p tower-acc-sim
# open http://127.0.0.1:3000
```

## How it works

The simulator models a client–server system where:

- A **client** generates requests as a Poisson process at a configurable rate
  and runs its own ACC algorithm for client-side throttling.
- A **server** accepts requests up to its ACC-computed concurrency limit,
  enqueues overflow into a bounded queue, and load-sheds when the queue is full.
- **Latency regimes** change over simulated time, letting you observe how each
  algorithm adapts to shifts in server processing time.

The simulation runs to completion in Rust (typically sub-second even at high
request rates), then returns the full trace as JSON. The frontend animates
playback at a user-chosen speed using [Plotly.js](https://plotly.com/javascript/).

## Charts

| Chart | What it shows |
|-------|---------------|
| **Concurrency Limits** | Server limit, client limit, and theoretical concurrency (Little's Law) |
| **Server: Limit vs Inflight** | How close the server is to its computed limit |
| **Client: Limit vs Inflight** | How close the client is to its computed limit |
| **Latency** | Observed latency (EMA) vs current regime's mean processing time |
| **Load Shedding & Queue** | Cumulative shed count, queue depth, and queue capacity |

## Configuration

All parameters are editable in the UI. The config is sent as JSON to
`POST /api/simulate`.

### Simulation

| Parameter | Default | Description |
|-----------|---------|-------------|
| Duration | 60s | Total simulated time |
| Request rate | 200 rps | Average requests per second (Poisson process) |
| Seed | 42 | RNG seed for deterministic runs |
| Queue capacity | 100 | Server queue size before load shedding |

### Algorithms

Both server and client independently choose an algorithm.

**AIMD** — loss-based (like TCP Reno). Increases by 1 on success, multiplies by
backoff ratio on error or timeout. Simple but reactive only to failures, not
latency.

| Parameter | Default |
|-----------|---------|
| Initial limit | 20 |
| Min / Max limit | 10 / 200 |
| Backoff ratio | 0.9 |
| Timeout | 1500ms |

**Gradient2** — gradient-based (from Netflix's [concurrency-limits]). Compares
long-term (EMA) RTT to short-term RTT. A configurable tolerance allows moderate
latency increases without reducing the limit.

| Parameter | Default |
|-----------|---------|
| Initial limit | 20 |
| Min / Max limit | 20 / 200 |
| Smoothing | 0.2 |
| RTT tolerance | 1.5 |
| Long window | 600 samples |

**Vegas** — queue-depth estimation (like TCP Vegas). Tracks minimum RTT as
baseline and estimates queue depth from the ratio of current RTT to baseline.

| Parameter | Default |
|-----------|---------|
| Initial limit | 20 |
| Max limit | 200 |
| Smoothing | 1.0 (no smoothing) |

### Default latency regimes

The default scenario walks through six phases:

| Time | Regime | Mean latency | Std factor | What happens |
|------|--------|-------------|------------|--------------|
| 0–10s | Easy | 50ms | 0.3 | Limits settle, no queuing |
| 10–20s | Moderate stress | 150ms | 0.4 | Limits climb, light queuing |
| 20–30s | Recovery | 50ms | 0.3 | Limits drop, queue drains |
| 30–42s | Dramatic spike | 500ms | 0.8 | Fat tails overwhelm, queue fills, shedding |
| 42–52s | Stressed but stable | 250ms | 0.35 | Partial queue, no shedding |
| 52–60s | Easy again | 50ms | 0.3 | Full recovery |

## API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Serves the single-page UI |
| `/api/defaults` | GET | Returns the default `SimConfig` as JSON |
| `/api/simulate` | POST | Accepts a `SimConfig` JSON body, returns the trace |

[concurrency-limits]: https://github.com/Netflix/concurrency-limits
