//! An Axum server with adaptive concurrency limiting using Gradient2 and a
//! custom HTTP status classifier.
//!
//! Demonstrates server-side Gradient2 (latency-gradient–based) concurrency
//! control with a classifier that distinguishes client errors (4xx) from server
//! errors (5xx). Only 5xx responses count as errors for limit adjustment.
//!
//! ```sh
//! cargo run --example axum
//! ```
//!
//! Then hit it with:
//!
//! ```sh
//! curl http://localhost:3000/
//! curl http://localhost:3000/not-found
//! ```

use std::time::Duration;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_acc::{Classifier, ConcurrencyLimitLayer, Gradient2};

/// Treats only 5xx responses as server errors for concurrency control.
/// 4xx errors (client mistakes) should not reduce the concurrency limit.
#[derive(Clone)]
struct HttpStatusClassifier;

impl<E> Classifier<axum::response::Response, E> for HttpStatusClassifier {
    fn is_server_error(&self, result: &Result<axum::response::Response, E>) -> bool {
        match result {
            Ok(response) => response.status().is_server_error(),
            Err(_) => true,
        }
    }
}

async fn handler() -> &'static str {
    // Simulate some work.
    tokio::time::sleep(Duration::from_millis(50)).await;
    "Hello, world!\n"
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found\n")
}

#[tokio::main]
async fn main() {
    let algorithm = Gradient2::builder()
        .initial_limit(10)
        .min_limit(1)
        .max_limit(100)
        .rtt_tolerance(1.5)
        .build();

    let app = Router::new()
        .route("/", get(handler))
        .route("/not-found", get(not_found))
        .layer(
            ServiceBuilder::new()
                .layer(ConcurrencyLimitLayer::with_classifier(algorithm, HttpStatusClassifier)),
        );

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://localhost:3000");
    println!("Algorithm: Gradient2 (latency-gradient), classifier: 5xx only");
    axum::serve(listener, app).await.unwrap();
}
