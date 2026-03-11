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
