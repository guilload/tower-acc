# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-03-11

### Added

- `Classifier<T, E>` trait for customizing which responses are treated as
  server errors for concurrency-control purposes.
- `DefaultClassifier` — preserves the previous behavior (`result.is_err()`).
- Blanket `Classifier` impl for `Fn(&Result<T, E>) -> bool` closures.
- `ConcurrencyLimit::with_classifier` and
  `ConcurrencyLimitLayer::with_classifier` constructors.

### Changed

- `ConcurrencyLimit` is now `ConcurrencyLimit<S, A, C = DefaultClassifier>`.
- `ConcurrencyLimitLayer` is now `ConcurrencyLimitLayer<A, C = DefaultClassifier>`.
- `ResponseFuture` is now `ResponseFuture<F, A, C>` (stores the classifier).
- `tower-layer` is now an optional dependency behind the `layer` feature flag
  (enabled by default). Use `default-features = false` to drop it.
- Set MSRV to 1.85.0 (`rust-version` in `Cargo.toml`).

## [0.1.0] - 2026-03-10

Initial release.

### Added

- `ConcurrencyLimit<S, A>` service wrapper with adaptive concurrency control.
- `ConcurrencyLimitLayer` for use with `tower::ServiceBuilder` (behind the
  `layer` feature, enabled by default).
- `Algorithm` trait for pluggable concurrency control strategies.
- `Vegas` algorithm (TCP Vegas-inspired adaptive limiting) with configurable
  thresholds via `VegasBuilder`.

[Unreleased]: https://github.com/guilload/tower-acc/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/guilload/tower-acc/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/guilload/tower-acc/releases/tag/v0.1.0
