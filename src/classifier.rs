/// Inspects the result of a service call to determine whether the outcome
/// should be treated as a *server error* for concurrency-control purposes.
///
/// By default ([`DefaultClassifier`]), any `Err` variant is considered a server
/// error. Implement this trait to distinguish client errors, expected failures,
/// or successful-but-bad responses (e.g. HTTP 503) from true server errors.
pub trait Classifier<T, E> {
    fn is_server_error(&self, result: &Result<T, E>) -> bool;
}

/// Blanket impl: any closure `Fn(&Result<T, E>) -> bool` works as a classifier.
impl<F, T, E> Classifier<T, E> for F
where
    F: Fn(&Result<T, E>) -> bool,
{
    fn is_server_error(&self, result: &Result<T, E>) -> bool {
        (self)(result)
    }
}

/// Classifies responses based on HTTP status code.
///
/// Any response with a 5xx status code is treated as a server error. Client
/// errors (4xx), redirects (3xx), and successful responses (2xx) are **not**
/// considered server errors and will not trigger a concurrency limit decrease.
///
/// `Err` variants are always treated as server errors.
///
/// This classifier is generic over the response body type, so it works with
/// any framework built on [`http::Response`] — including axum, warp, tonic,
/// and hyper.
///
/// # Example
///
/// ```rust
/// use tower::ServiceBuilder;
/// use tower_acc::{ConcurrencyLimitLayer, HttpStatusClassifier, Vegas};
/// # fn wrap<S>(my_service: S) -> impl tower_service::Service<()>
/// # where S: tower_service::Service<(), Response = http::Response<()>, Error = std::convert::Infallible> {
///
/// let service = ServiceBuilder::new()
///     .layer(ConcurrencyLimitLayer::with_classifier(
///         Vegas::default(),
///         HttpStatusClassifier,
///     ))
///     .service(my_service);
/// # service
/// # }
/// ```
///
/// Requires the `http` feature (enabled separately).
#[cfg(feature = "http")]
#[derive(Clone, Debug, Default)]
pub struct HttpStatusClassifier;

#[cfg(feature = "http")]
impl<B, E> Classifier<http::Response<B>, E> for HttpStatusClassifier {
    fn is_server_error(&self, result: &Result<http::Response<B>, E>) -> bool {
        match result {
            Ok(response) => response.status().is_server_error(),
            Err(_) => true,
        }
    }
}

/// The default classifier: treats every `Err` as a server error.
///
/// This preserves the original behavior where `result.is_err()` was used
/// directly.
#[derive(Clone, Debug, Default)]
pub struct DefaultClassifier;

impl<T, E> Classifier<T, E> for DefaultClassifier {
    fn is_server_error(&self, result: &Result<T, E>) -> bool {
        result.is_err()
    }
}
