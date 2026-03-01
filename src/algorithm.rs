use std::time::Duration;

/// An algorithm that dynamically adjusts the maximum number of allowed concurrent requests based on the observed traffic.
pub trait Algorithm {
    /// Returns the maximum number of concurrent requests the algorithm currently allows.
    ///
    /// # Panics
    ///
    /// Implementations **must not** panic. This method is called while holding
    /// a shared mutex; a panic would poison it and abort on the next request.
    fn max_concurrency(&self) -> usize;

    /// Observes the outcome of a request and updates the algorithm's state accordingly.
    ///
    /// # Panics
    ///
    /// Implementations **must not** panic. This method is called while holding
    /// a shared mutex; a panic would poison it and abort on the next request.
    fn update(&mut self, rtt: Duration, is_error: bool, is_canceled: bool);
}
