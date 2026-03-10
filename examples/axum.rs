//! An Axum server with adaptive concurrency limiting.
//!
//! Demonstrates the simplest integration: apply `ConcurrencyLimitLayer` via
//! `Router::layer`. Each connection shares the same adaptive concurrency limit.
//!
//! For a full load-shedding example (Buffer + LoadShed), see the `hyper`
//! example, which gives explicit control over the shared middleware stack.
//!
//! ```sh
//! cargo run --example axum
//! ```
//!
//! Then hit it with:
//!
//! ```sh
//! curl http://localhost:3000/
//! ```

use std::time::Duration;

use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_acc::{ConcurrencyLimitLayer, Vegas};

async fn handler() -> &'static str {
    // Simulate some work.
    tokio::time::sleep(Duration::from_millis(50)).await;
    "Hello, world!\n"
}

#[tokio::main]
async fn main() {
    let algorithm = Vegas::builder().initial_limit(10).max_limit(100).build();

    let app = Router::new()
        .route("/", get(handler))
        .layer(ServiceBuilder::new().layer(ConcurrencyLimitLayer::new(algorithm)));

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}
