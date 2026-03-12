//! A Tower service with AIMD concurrency limiting, a custom classifier,
//! buffering, and load shedding.
//!
//! Demonstrates client-side AIMD (loss-based, TCP Reno–style) concurrency
//! control. AIMD reacts to errors and timeouts by multiplicatively decreasing
//! the limit, and additively increases it on success — making it a good fit
//! for clients that want aggressive backoff on failures.
//!
//! A custom classifier treats "not_found" errors as client mistakes (not server
//! errors), so they don't reduce the concurrency limit.
//!
//! The middleware stack (outermost to innermost):
//!
//! 1. **LoadShed** — when the buffer is full, immediately rejects the request.
//! 2. **Buffer** — queues up to `BUFFER_SIZE` requests in front of the
//!    concurrency limiter.
//! 3. **ConcurrencyLimit (AIMD)** — controls how many requests are issued
//!    concurrently, adjusting the limit based on errors and timeouts.
//!
//! ```sh
//! cargo run --example tower
//! ```

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tower::{Service, ServiceBuilder, ServiceExt};
use tower_acc::{Aimd, ConcurrencyLimitLayer};

/// Maximum number of requests waiting in the buffer before load shedding kicks
/// in.
const BUFFER_SIZE: usize = 8;

/// A toy service that "processes" a request by sleeping, then echoing it back.
/// Returns an error for requests containing "fail" or "not_found".
#[derive(Clone)]
struct Echo;

impl Service<String> for Echo {
    type Response = String;
    type Error = String;
    type Future = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: String) -> Self::Future {
        Box::pin(async move {
            // Simulate work.
            tokio::time::sleep(Duration::from_millis(50)).await;

            if req.contains("fail") {
                Err("server_error".to_string())
            } else if req.contains("not_found") {
                Err("not_found".to_string())
            } else {
                Ok(format!("echo: {req}"))
            }
        })
    }
}

#[tokio::main]
async fn main() {
    let algorithm = Aimd::builder()
        .initial_limit(2)
        .min_limit(1)
        .max_limit(10)
        .backoff_ratio(0.9)
        .timeout(Duration::from_secs(2))
        .build();

    // Classifier: "not_found" errors are client mistakes, not server errors.
    // Only actual server errors should reduce the concurrency limit.
    let classifier = |result: &Result<String, String>| match result {
        Ok(_) => false,
        Err(err) => err != "not_found",
    };

    let svc = ServiceBuilder::new()
        // 1. Shed load: reject immediately when the buffer is full.
        .load_shed()
        // 2. Buffer: queue up to BUFFER_SIZE requests.
        .buffer(BUFFER_SIZE)
        // 3. Adaptive concurrency limit with AIMD + classifier.
        .layer(ConcurrencyLimitLayer::with_classifier(algorithm, classifier))
        .service(Echo);

    // Fire a mix of requests: normal, not_found (client error), and fail (server error).
    let requests = vec![
        "request 0", "request 1", "not_found 2", "request 3", "fail 4",
        "request 5", "request 6", "not_found 7", "request 8", "request 9",
        "fail 10", "request 11", "request 12", "request 13", "not_found 14",
        "request 15", "request 16", "request 17", "request 18", "request 19",
    ];

    let mut handles = Vec::new();
    for req in requests {
        let mut svc = svc.clone();
        let req = req.to_string();
        handles.push(tokio::spawn(async move {
            match svc.ready().await {
                Ok(svc) => match svc.call(req.clone()).await {
                    Ok(resp) => println!("  ok: {resp}"),
                    Err(err) => println!(" err: {req} ({err})"),
                },
                Err(err) => println!("shed: {req} ({err})"),
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}
