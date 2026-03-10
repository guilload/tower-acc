# tower-acc

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

## How it works

The default algorithm is **Vegas**, inspired by the TCP Vegas congestion control
scheme. It works by tracking the minimum observed RTT (the "no-load" baseline)
and comparing each request's RTT against it:

1. **Estimate queue depth** — `limit × (1 − rtt_noload / rtt)`.
2. **If the queue is short** (below alpha) — increase the limit.
3. **If the queue is long** (above beta) — decrease the limit.
4. **On errors** — decrease immediately.
5. **Periodically probe** — reset the baseline to track changing conditions.

All thresholds are configurable via `VegasBuilder`.

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

### Custom configuration

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

### Pluggable algorithms

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
