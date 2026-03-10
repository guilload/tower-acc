use crate::Algorithm;
use crate::controller::Controller;
use crate::future::ResponseFuture;

use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::PollSemaphore;
use tower_service::Service;

use std::{
    sync::{Arc, Mutex},
    task::{Context, Poll, ready},
    time::Instant,
};

/// Enforces an adaptive limit on the concurrent number of requests the
/// underlying service can handle.
///
/// Unlike a static concurrency limit, `ConcurrencyLimit` continuously observes
/// request latency and adjusts the number of allowed in-flight requests using
/// the configured [`Algorithm`].
///
/// Use [`ConcurrencyLimitLayer`](crate::ConcurrencyLimitLayer) to integrate
/// with `tower::ServiceBuilder`.
pub struct ConcurrencyLimit<S, A> {
    inner: S,
    controller: Arc<Mutex<Controller<A>>>,
    semaphore: PollSemaphore,
    /// The currently acquired semaphore permit, if there is sufficient
    /// concurrency to send a new request.
    ///
    /// The permit is acquired in `poll_ready`, and taken in `call` when sending
    /// a new request.
    permit: Option<OwnedSemaphorePermit>,
}

impl<S, A> ConcurrencyLimit<S, A>
where
    A: Algorithm,
{
    /// Creates a new concurrency limiter.
    pub fn new(inner: S, algorithm: A) -> Self {
        let controller = Controller::new(algorithm);
        let semaphore = controller.semaphore();

        Self {
            inner,
            controller: Arc::new(Mutex::new(controller)),
            semaphore: PollSemaphore::new(semaphore),
            permit: None,
        }
    }

    /// Gets a reference to the inner service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Gets a mutable reference to the inner service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consumes `self`, returning the inner service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, A, Request> Service<Request> for ConcurrencyLimit<S, A>
where
    S: Service<Request>,
    A: Algorithm,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, A>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.permit.is_none() {
            self.permit = ready!(self.semaphore.poll_acquire(cx));
            debug_assert!(self.permit.is_some(), "semaphore should never be closed");
        }
        // Once we've acquired a permit (or if we already had one), poll the
        // inner service.
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let start = Instant::now();
        // Take the permit
        let permit = self
            .permit
            .take()
            .expect("`poll_ready` should be called first");

        // Call the inner service
        let future = self.inner.call(request);
        ResponseFuture::new(future, self.controller.clone(), permit, start)
    }
}

impl<S: Clone, A> Clone for ConcurrencyLimit<S, A> {
    fn clone(&self) -> Self {
        // Since we hold an `OwnedSemaphorePermit`, we can't derive `Clone`.
        // Instead, when cloning the service, create a new service with the
        // same semaphore, but with the permit in the un-acquired state.
        Self {
            inner: self.inner.clone(),
            controller: self.controller.clone(),
            semaphore: self.semaphore.clone(),
            permit: None,
        }
    }
}

impl<S: std::fmt::Debug, A> std::fmt::Debug for ConcurrencyLimit<S, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrencyLimit")
            .field("inner", &self.inner)
            .field("permit", &self.permit)
            .finish_non_exhaustive()
    }
}
