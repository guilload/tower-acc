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
//! # Pluggable algorithms
//!
//! The built-in [`Vegas`] algorithm works well for most workloads. To implement
//! a custom strategy, see the [`Algorithm`] trait.
//!
//! [Tower]: https://github.com/tower-rs/tower
//! [concurrency-limits]: https://github.com/Netflix/concurrency-limits

mod aimd;
mod algorithm;
mod controller;
mod future;
#[cfg(feature = "layer")]
mod layer;
mod service;
mod vegas;

pub use self::aimd::Aimd;
pub use self::algorithm::Algorithm;
#[cfg(feature = "layer")]
pub use self::layer::ConcurrencyLimitLayer;
pub use self::service::ConcurrencyLimit;
pub use self::vegas::Vegas;
