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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::Controller;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Semaphore;

    /// Algorithm that records every `update` call.
    struct RecordingAlgorithm {
        limit: usize,
        updates: Arc<Mutex<Vec<(bool, bool)>>>, // (is_error, is_canceled)
    }

    impl RecordingAlgorithm {
        fn new(limit: usize) -> (Self, Arc<Mutex<Vec<(bool, bool)>>>) {
            let updates = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    limit,
                    updates: updates.clone(),
                },
                updates,
            )
        }
    }

    impl Algorithm for RecordingAlgorithm {
        fn max_concurrency(&self) -> usize {
            self.limit
        }

        fn update(
            &mut self,
            _rtt: Duration,
            _num_inflight: usize,
            is_error: bool,
            is_canceled: bool,
        ) {
            self.updates.lock().unwrap().push((is_error, is_canceled));
        }
    }

    fn make_fixture(
        limit: usize,
    ) -> (
        Arc<Mutex<Controller<RecordingAlgorithm>>>,
        Arc<Semaphore>,
        Arc<Mutex<Vec<(bool, bool)>>>,
    ) {
        let (algo, updates) = RecordingAlgorithm::new(limit);
        let controller = Controller::new(algo);
        let semaphore = controller.semaphore();
        (Arc::new(Mutex::new(controller)), semaphore, updates)
    }

    #[tokio::test]
    async fn success_reports_no_error_no_cancel() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        let fut = ResponseFuture::new(
            async { Ok::<_, ()>("ok") },
            controller,
            permit,
            Instant::now(),
        );

        let result = fut.await;
        assert!(result.is_ok());

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (false, false)); // no error, not canceled
    }

    #[tokio::test]
    async fn error_reports_is_error() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        let fut = ResponseFuture::new(
            async { Err::<(), _>("fail") },
            controller,
            permit,
            Instant::now(),
        );

        let result = fut.await;
        assert!(result.is_err());

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (true, false)); // error, not canceled
    }

    #[tokio::test]
    async fn drop_before_completion_reports_canceled() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        // Create a future that will never resolve.
        let fut = ResponseFuture::new(
            std::future::pending::<Result<(), ()>>(),
            controller,
            permit,
            Instant::now(),
        );

        // Drop without polling to completion — should report canceled.
        drop(fut);

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (false, true)); // no error, canceled
    }

    #[tokio::test]
    async fn permit_returned_before_controller_update() {
        let (controller, semaphore, updates) = make_fixture(1);

        // Acquire the only permit.
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        assert_eq!(semaphore.available_permits(), 0);

        let fut = ResponseFuture::new(
            async { Ok::<_, ()>("ok") },
            controller,
            permit,
            Instant::now(),
        );

        fut.await.unwrap();

        // After completion + drop, the permit should be returned.
        assert_eq!(semaphore.available_permits(), 1);

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
    }

    #[tokio::test]
    async fn pending_then_ready() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        // A future that returns Pending once, then Ready(Ok).
        struct OnePending {
            polled: bool,
        }

        impl Future for OnePending {
            type Output = Result<&'static str, ()>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.get_mut();
                if !this.polled {
                    this.polled = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(Ok("done"))
                }
            }
        }

        let fut = ResponseFuture::new(
            OnePending { polled: false },
            controller,
            permit,
            Instant::now(),
        );

        let result = fut.await;
        assert_eq!(result.unwrap(), "done");

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (false, false));
    }
}
