//! Adaptive concurrency control for [Tower] services.
//!
//! This crate provides a [`ConcurrencyLimit`] middleware that dynamically
//! adjusts the number of in-flight requests based on observed latency, rather
//! than requiring a fixed limit chosen at deploy time. It is inspired by
//! Netflix's [concurrency-limits] library and the TCP Vegas congestion control
//! algorithm.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use tower::ServiceBuilder;
//! use tower_acc::{ConcurrencyLimitLayer, Vegas};
//! # fn wrap<S>(my_service: S) -> impl tower_service::Service<()>
//! # where S: tower_service::Service<(), Error = std::convert::Infallible> {
//!
//! let service = ServiceBuilder::new()
//!     .layer(ConcurrencyLimitLayer::new(Vegas::default()))
//!     .service(my_service);
//! # service
//! # }
//! ```
//!
//! # Algorithms
//!
//! Three built-in algorithms are provided:
//!
//! - [`Aimd`] — loss-based (TCP Reno–style). Reacts to errors and timeouts.
//! - [`Gradient2`] — gradient-based (Netflix-style). Compares long-term vs
//!   short-term RTT with a configurable tolerance.
//! - [`Vegas`] — queue-depth estimation (TCP Vegas–style). Tracks minimum RTT
//!   as a no-load baseline.
//!
//! To implement a custom strategy, see the [`Algorithm`] trait.
//!
//! [Tower]: https://github.com/tower-rs/tower
//! [concurrency-limits]: https://github.com/Netflix/concurrency-limits

mod aimd;
mod algorithm;
mod classifier;
mod controller;
mod future;
mod gradient2;
#[cfg(feature = "layer")]
mod layer;
mod service;
mod sync;
mod vegas;

pub use self::aimd::Aimd;
pub use self::algorithm::Algorithm;
pub use self::classifier::{Classifier, DefaultClassifier};
pub use self::gradient2::Gradient2;
#[cfg(feature = "layer")]
pub use self::layer::ConcurrencyLimitLayer;
pub use self::service::ConcurrencyLimit;
pub use self::vegas::Vegas;
