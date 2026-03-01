use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Instant,
};
use tokio::sync::OwnedSemaphorePermit;

use crate::Algorithm;
use crate::controller::Controller;

struct FutureGuard<A: Algorithm> {
    controller: Arc<Mutex<Controller<A>>>,
    is_canceled: bool,
    is_error: bool,
    permit: Option<OwnedSemaphorePermit>,
    start: Instant,
}

impl<A: Algorithm> Drop for FutureGuard<A> {
    fn drop(&mut self) {
        // Return the permit to the semaphore before resizing so that
        // `forget_permits` has the maximum number of available permits
        // to consume.
        drop(self.permit.take());

        self.controller
            .lock()
            .expect("Controller::update should not panic")
            .update(self.start.elapsed(), self.is_error, self.is_canceled);
    }
}

pin_project! {
    pub struct ResponseFuture<F, A: Algorithm> {
        #[pin]
        future: F,
        guard: FutureGuard<A>,
    }
}

impl<F, A: Algorithm> ResponseFuture<F, A> {
    pub(super) fn new(
        future: F,
        controller: Arc<Mutex<Controller<A>>>,
        permit: OwnedSemaphorePermit,
        start: Instant,
    ) -> Self {
        Self {
            future,
            guard: FutureGuard {
                controller,
                is_canceled: true,
                is_error: false,
                permit: Some(permit),
                start,
            },
        }
    }
}

impl<F, T, E, A> Future for ResponseFuture<F, A>
where
    F: Future<Output = Result<T, E>>,
    A: Algorithm,
{
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.future.poll(cx) {
            Poll::Ready(result) => {
                this.guard.is_canceled = false;
                this.guard.is_error = result.is_err();
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
