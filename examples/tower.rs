//! A Tower service with adaptive concurrency limiting, buffering, and load
//! shedding.
//!
//! The middleware stack (outermost to innermost):
//!
//! 1. **LoadShed** — when the buffer is full, immediately rejects the request.
//! 2. **Buffer** — queues up to `BUFFER_SIZE` requests in front of the
//!    concurrency limiter.
//! 3. **ConcurrencyLimit (adaptive)** — controls how many requests reach the
//!    handler concurrently, adjusting the limit based on observed latency.
//!
//! ```sh
//! cargo run --example tower
//! ```

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tower::{Service, ServiceBuilder, ServiceExt};
use tower_acc::{ConcurrencyLimitLayer, Vegas};

/// Maximum number of requests waiting in the buffer before load shedding kicks
/// in.
const BUFFER_SIZE: usize = 8;

/// A toy service that "processes" a request by sleeping, then echoing it back.
#[derive(Clone)]
struct Echo;

impl Service<String> for Echo {
    type Response = String;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<String, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        Box::pin(async move {
            // Simulate work.
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(format!("echo: {req}"))
        })
    }
}

#[tokio::main]
async fn main() {
    let algorithm = Vegas::builder().initial_limit(2).max_limit(10).build();

    let svc = ServiceBuilder::new()
        // 1. Shed load: reject immediately when the buffer is full.
        .load_shed()
        // 2. Buffer: queue up to BUFFER_SIZE requests.
        .buffer(BUFFER_SIZE)
        // 3. Adaptive concurrency limit.
        .layer(ConcurrencyLimitLayer::new(algorithm))
        .service(Echo);

    // Fire 20 concurrent requests to trigger load shedding.
    let mut handles = Vec::new();
    for i in 0..20 {
        let mut svc = svc.clone();
        handles.push(tokio::spawn(async move {
            match svc.ready().await {
                Ok(svc) => match svc.call(format!("request {i}")).await {
                    Ok(resp) => println!("  ok: {resp}"),
                    Err(err) => println!("shed: request {i} ({err})"),
                },
                Err(err) => println!("shed: request {i} ({err})"),
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}
