//! A Hyper server with Gradient2 concurrency limiting, a custom classifier,
//! buffering, and load shedding.
//!
//! The middleware stack (outermost to innermost):
//!
//! 1. **LoadShed** — when the buffer is full, immediately rejects the request.
//! 2. **Buffer** — queues up to `BUFFER_SIZE` requests in front of the
//!    concurrency limiter.
//! 3. **ConcurrencyLimit (Gradient2)** — controls how many requests reach the
//!    handler concurrently, adjusting the limit based on the latency gradient.
//!    A custom classifier treats only 5xx as server errors.
//!
//! ```sh
//! cargo run --example hyper
//! ```
//!
//! Then flood it:
//!
//! ```sh
//! # With hey (https://github.com/rakyll/hey):
//! hey -n 500 -c 100 http://localhost:3000/
//! ```
//!
//! You should see some requests succeed (200) while excess requests are shed
//! (503) once the buffer fills up.

use std::convert::Infallible;
use std::time::Duration;

use http::{Request, Response, StatusCode};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use tokio::net::TcpListener;
use tower::{Service, ServiceBuilder, ServiceExt};
use tower_acc::{ConcurrencyLimitLayer, Gradient2};

/// Maximum number of requests waiting in the buffer before load shedding kicks
/// in. Intentionally small so that shedding is easy to trigger under load.
const BUFFER_SIZE: usize = 16;

async fn handler(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    // Simulate slow backend work (200ms).
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok(Response::new(Full::new(Bytes::from("Hello, world!\n"))))
}

#[tokio::main]
async fn main() {
    let algorithm = Gradient2::builder()
        .initial_limit(5)
        .min_limit(1)
        .max_limit(20)
        .rtt_tolerance(1.5)
        .build();

    // Classifier: only treat 5xx responses as server errors. Load-shed 503s
    // from the outer middleware are transport errors (Err), so they're always
    // counted — but successful 4xx responses won't penalize the limit.
    let classifier = |result: &Result<Response<Full<Bytes>>, Infallible>| match result {
        Ok(resp) => resp.status().is_server_error(),
        Err(_) => true,
    };

    let svc = ServiceBuilder::new()
        // 1. Shed load: reject immediately when the buffer is full.
        .load_shed()
        // 2. Buffer: queue up to BUFFER_SIZE requests.
        .buffer(BUFFER_SIZE)
        // 3. Adaptive concurrency limit with Gradient2 + classifier.
        .layer(ConcurrencyLimitLayer::with_classifier(
            algorithm, classifier,
        ))
        .service_fn(handler);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://localhost:3000");
    println!("Algorithm: Gradient2, buffer: {BUFFER_SIZE}, initial limit: 5, max: 20");

    loop {
        let (stream, _addr) = listener.accept().await.unwrap();
        let svc = svc.clone();

        tokio::spawn(async move {
            let hyper_svc = hyper::service::service_fn(move |req: Request<Incoming>| {
                let mut svc = svc.clone();
                async move {
                    match svc.ready().await {
                        Ok(svc) => match svc.call(req).await {
                            Ok(resp) => Ok::<_, Infallible>(resp),
                            Err(err) => Ok(error_response(err)),
                        },
                        Err(err) => Ok(error_response(err)),
                    }
                }
            });

            let result = Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), hyper_svc)
                .await;

            if let Err(err) = result {
                eprintln!("Connection error: {err}");
            }
        });
    }
}

/// Converts a tower middleware error into a proper HTTP response.
fn error_response(err: tower::BoxError) -> Response<Full<Bytes>> {
    if err.is::<tower::load_shed::error::Overloaded>() {
        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::from("service unavailable")))
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from(format!(
                "internal server error: {err}"
            ))))
            .unwrap()
    }
}
