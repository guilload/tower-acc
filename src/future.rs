use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use crate::sync::{Arc, Mutex};
use tokio::sync::OwnedSemaphorePermit;

use crate::Algorithm;
use crate::classifier::Classifier;
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

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("acc.update_controller").entered();

        self.controller
            .lock()
            .expect("Controller::update should not panic")
            .update(self.start.elapsed(), self.is_error, self.is_canceled);
    }
}

pin_project! {
    pub struct ResponseFuture<F, A: Algorithm, C> {
        #[pin]
        future: F,
        classifier: C,
        guard: FutureGuard<A>,
    }
}

impl<F, A: Algorithm, C> ResponseFuture<F, A, C> {
    pub(super) fn new(
        future: F,
        controller: Arc<Mutex<Controller<A>>>,
        permit: OwnedSemaphorePermit,
        start: Instant,
        classifier: C,
    ) -> Self {
        Self {
            future,
            classifier,
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

impl<F, T, E, A, C> Future for ResponseFuture<F, A, C>
where
    F: Future<Output = Result<T, E>>,
    A: Algorithm,
    C: Classifier<T, E>,
{
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.future.poll(cx) {
            Poll::Ready(result) => {
                this.guard.is_canceled = false;
                this.guard.is_error = this.classifier.is_server_error(&result);
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::DefaultClassifier;
    use crate::controller::Controller;
    use crate::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Semaphore;

    type UpdateLog = Arc<Mutex<Vec<(bool, bool)>>>;

    /// Algorithm that records every `update` call.
    struct RecordingAlgorithm {
        limit: usize,
        updates: UpdateLog, // (is_error, is_canceled)
    }

    impl RecordingAlgorithm {
        fn new(limit: usize) -> (Self, UpdateLog) {
            let updates: UpdateLog = Arc::new(Mutex::new(Vec::new()));
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
        UpdateLog,
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
            DefaultClassifier,
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
            DefaultClassifier,
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
            DefaultClassifier,
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
            DefaultClassifier,
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
            DefaultClassifier,
        );

        let result = fut.await;
        assert_eq!(result.unwrap(), "done");

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (false, false));
    }

    #[tokio::test]
    async fn custom_classifier_overrides_error_detection() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        // Classifier that treats Err("not_found") as NOT a server error.
        let classifier = |result: &Result<(), &str>| match result {
            Err(e) => *e != "not_found",
            Ok(_) => false,
        };

        let fut = ResponseFuture::new(
            async { Err::<(), _>("not_found") },
            controller,
            permit,
            Instant::now(),
            classifier,
        );

        let result = fut.await;
        assert!(result.is_err());

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (false, false)); // NOT a server error
    }

    #[tokio::test]
    async fn classifier_can_inspect_ok_variant() {
        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        // Classifier that treats Ok(503) as a server error.
        let classifier = |result: &Result<u16, &str>| match result {
            Ok(status) => *status >= 500,
            Err(_) => true,
        };

        let fut = ResponseFuture::new(
            async { Ok::<u16, &str>(503) },
            controller,
            permit,
            Instant::now(),
            classifier,
        );

        let result = fut.await;
        assert_eq!(result.unwrap(), 503);

        let log = updates.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], (true, false)); // IS a server error
    }

    #[tokio::test]
    async fn struct_classifier() {
        struct HttpClassifier;

        impl Classifier<u16, &'static str> for HttpClassifier {
            fn is_server_error(&self, result: &Result<u16, &'static str>) -> bool {
                match result {
                    Ok(status) => *status >= 500,
                    Err(_) => true,
                }
            }
        }

        let (controller, semaphore, updates) = make_fixture(10);
        let permit = semaphore.acquire_owned().await.unwrap();

        let fut = ResponseFuture::new(
            async { Ok::<u16, &str>(200) },
            controller,
            permit,
            Instant::now(),
            HttpClassifier,
        );

        let result = fut.await;
        assert_eq!(result.unwrap(), 200);

        let log = updates.lock().unwrap();
        assert_eq!(log[0], (false, false)); // 200 is not a server error
    }
}
