use tower_layer::Layer;

use crate::Algorithm;
use crate::service::ConcurrencyLimit;

/// A [`Layer`] that wraps services with an adaptive [`ConcurrencyLimit`].
///
/// # Example
///
/// ```rust,no_run
/// use tower::ServiceBuilder;
/// use tower_acc::{ConcurrencyLimitLayer, Vegas};
/// # fn wrap<S>(my_service: S) -> impl tower_service::Service<()>
/// # where S: tower_service::Service<(), Error = std::convert::Infallible> {
///
/// let service = ServiceBuilder::new()
///     .layer(ConcurrencyLimitLayer::new(Vegas::default()))
///     .service(my_service);
/// # service
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ConcurrencyLimitLayer<A> {
    algorithm: A,
}

impl<A> ConcurrencyLimitLayer<A> {
    /// Creates a new `ConcurrencyLimitLayer` with the given algorithm.
    pub fn new(algorithm: A) -> Self {
        Self { algorithm }
    }
}

impl<S, A> Layer<S> for ConcurrencyLimitLayer<A>
where
    A: Algorithm + Clone,
{
    type Service = ConcurrencyLimit<S, A>;

    fn layer(&self, service: S) -> Self::Service {
        ConcurrencyLimit::new(service, self.algorithm.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::future::{Ready, ready};
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tower_service::Service;

    /// Minimal algorithm with a fixed limit.
    #[derive(Clone, Debug)]
    struct FixedAlgorithm(usize);

    impl Algorithm for FixedAlgorithm {
        fn max_concurrency(&self) -> usize {
            self.0
        }

        fn update(
            &mut self,
            _rtt: Duration,
            _num_inflight: usize,
            _is_error: bool,
            _is_canceled: bool,
        ) {
        }
    }

    /// Trivial service that returns the request unchanged.
    #[derive(Clone, Debug)]
    struct EchoService;

    impl Service<&'static str> for EchoService {
        type Response = &'static str;
        type Error = Infallible;
        type Future = Ready<Result<&'static str, Infallible>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: &'static str) -> Self::Future {
            ready(Ok(req))
        }
    }

    #[test]
    fn layer_produces_concurrency_limit_service() {
        let layer = ConcurrencyLimitLayer::new(FixedAlgorithm(10));
        let svc = layer.layer(EchoService);
        // Verify we get a ConcurrencyLimit wrapping EchoService.
        let inner: &EchoService = svc.get_ref();
        assert!(format!("{:?}", inner).contains("EchoService"));
    }

    #[tokio::test]
    async fn layered_service_forwards_requests() {
        let layer = ConcurrencyLimitLayer::new(FixedAlgorithm(10));
        let mut svc = layer.layer(EchoService);

        // poll_ready + call.
        std::future::poll_fn(|cx| svc.poll_ready(cx)).await.unwrap();
        let resp = svc.call("hello").await.unwrap();
        assert_eq!(resp, "hello");
    }

    #[test]
    fn layer_is_clone() {
        let layer = ConcurrencyLimitLayer::new(FixedAlgorithm(5));
        let layer2 = layer.clone();
        // Both produce working services.
        let _ = layer.layer(EchoService);
        let _ = layer2.layer(EchoService);
    }

    #[test]
    fn layer_is_debug() {
        let layer = ConcurrencyLimitLayer::new(FixedAlgorithm(5));
        let debug = format!("{:?}", layer);
        assert!(debug.contains("ConcurrencyLimitLayer"));
    }
}
