# tower-acc

[![Crates.io](https://img.shields.io/crates/v/tower-acc.svg)](https://crates.io/crates/tower-acc)
[![Documentation](https://docs.rs/tower-acc/badge.svg)](https://docs.rs/tower-acc)
[![CI](https://github.com/guilload/tower-acc/actions/workflows/ci.yml/badge.svg)](https://github.com/guilload/tower-acc/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/tower-acc.svg)](LICENSE)
[![MSRV](https://img.shields.io/crates/msrv/tower-acc.svg)](https://www.rust-lang.org)
[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](CODE_OF_CONDUCT.md)

Adaptive concurrency control for [Tower] services.

`tower-acc` dynamically adjusts the number of in-flight requests a service is
allowed to handle, based on observed latency. Instead of picking a fixed
concurrency limit and hoping it's right, it continuously measures round-trip
times and converges on the optimal limit automatically — increasing it when
latency is low and decreasing it when queuing is detected.

## Why not a static limit?

Tower ships with [`ConcurrencyLimit`][tower-cl], which caps concurrency at a
value you choose at startup. That works when the capacity of the downstream
service is known and stable, but in practice:

- Backends scale up and down.
- Dependency latency varies with load.
- The "right" limit depends on conditions you can't predict at deploy time.

Setting the limit too low wastes capacity; setting it too high causes queuing,
tail-latency spikes, and cascading failures under load. `tower-acc` removes the
guesswork by adapting the limit at runtime.

## Algorithms

Three built-in algorithms are provided. All are configurable through builder
APIs and implement the [`Algorithm`] trait.

### AIMD

A loss-based algorithm (like TCP Reno). Increases the limit by 1 on each
successful response and multiplies by a backoff ratio on errors or timeouts.
Simple and predictable, but only reacts to failures — not to latency changes.

```rust
use tower_acc::{ConcurrencyLimitLayer, Aimd};

let layer = ConcurrencyLimitLayer::new(
    Aimd::builder()
        .initial_limit(20)
        .min_limit(10)
        .max_limit(200)
        .backoff_ratio(0.9)
        .timeout(std::time::Duration::from_secs(5))
        .build(),
);
```

### Gradient2

Gradient-based algorithm inspired by Netflix's [concurrency-limits] library.
Compares long-term (exponentially smoothed) RTT against short-term RTT to detect
queueing. A configurable tolerance ratio allows moderate latency increases
without reducing the limit, making it more robust to natural variance than Vegas.

```rust
use tower_acc::{ConcurrencyLimitLayer, Gradient2};

let layer = ConcurrencyLimitLayer::new(
    Gradient2::builder()
        .initial_limit(20)
        .min_limit(20)
        .max_limit(200)
        .smoothing(0.2)
        .rtt_tolerance(1.5)
        .long_window(600)
        .build(),
);
```

### Vegas

Inspired by the TCP Vegas congestion control scheme. Tracks the minimum observed
RTT (the "no-load" baseline) and estimates queue depth from the ratio of current
RTT to baseline:

1. **Estimate queue depth** — `limit × (1 − rtt_noload / rtt)`.
2. **If the queue is short** (below alpha) — increase the limit.
3. **If the queue is long** (above beta) — decrease the limit.
4. **On errors** — decrease immediately.
5. **Periodically probe** — reset the baseline to track changing conditions.

```rust
use tower_acc::{ConcurrencyLimitLayer, Vegas};

let layer = ConcurrencyLimitLayer::new(
    Vegas::builder()
        .initial_limit(20)
        .max_limit(500)
        .smoothing(0.5)
        .build(),
);
```

## Usage

### As a Tower layer

```rust
use tower::ServiceBuilder;
use tower_acc::{ConcurrencyLimitLayer, Vegas};

let service = ServiceBuilder::new()
    .layer(ConcurrencyLimitLayer::new(Vegas::default()))
    .service(my_service);
```

### Wrapping a service directly

```rust
use tower_acc::{ConcurrencyLimit, Vegas};

let service = ConcurrencyLimit::new(my_service, Vegas::default());
```

### Custom algorithms

Implement the `Algorithm` trait to bring your own strategy:

```rust
use std::time::Duration;
use tower_acc::Algorithm;

struct MyAlgorithm { /* ... */ }

impl Algorithm for MyAlgorithm {
    fn max_concurrency(&self) -> usize {
        // Return the current concurrency limit.
        todo!()
    }

    fn update(&mut self, rtt: Duration, num_inflight: usize, is_error: bool, is_canceled: bool) {
        // Adjust internal state based on the observed request outcome.
        todo!()
    }
}
```

[`Algorithm`]: https://docs.rs/tower-acc/latest/tower_acc/trait.Algorithm.html

## Simulator

The [`tower-acc-sim`](tower-acc-sim/) crate provides an interactive web-based
simulator for exploring how the algorithms behave under changing server
conditions. See the [simulator README](tower-acc-sim/README.md) for details.

## Inspiration

This crate is a Rust/Tower port of the ideas from Netflix's
[concurrency-limits] library and the accompanying blog post
[Performance Under Load]. The core insight — applying TCP congestion control
theory to request-level concurrency — comes directly from that work.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for
details.

[Tower]: https://github.com/tower-rs/tower
[tower-cl]: https://docs.rs/tower/latest/tower/limit/concurrency/struct.ConcurrencyLimit.html
[concurrency-limits]: https://github.com/Netflix/concurrency-limits
[Performance Under Load]: https://netflixtechblog.medium.com/performance-under-load-3e6fa9a60581
