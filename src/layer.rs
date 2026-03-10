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
