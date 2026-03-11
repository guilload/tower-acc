#![cfg(feature = "loom")]

use loom::sync::{Arc, Mutex};
use loom::thread;
use std::cmp::Ordering;

/// A fake permit counter that models `tokio::sync::Semaphore` for loom testing.
/// Uses a `loom::sync::Mutex<usize>` instead of an actual async semaphore.
struct FakePermits {
    available: Mutex<usize>,
}

impl FakePermits {
    fn new(n: usize) -> Self {
        Self {
            available: Mutex::new(n),
        }
    }

    /// Model of `Semaphore::add_permits`.
    fn add_permits(&self, n: usize) {
        let mut avail = self.available.lock().unwrap();
        *avail += n;
    }

    /// Model of `Semaphore::forget_permits` — forgets up to `n` permits,
    /// returns how many were actually forgotten.
    fn forget_permits(&self, n: usize) -> usize {
        let mut avail = self.available.lock().unwrap();
        let forgotten = n.min(*avail);
        *avail -= forgotten;
        forgotten
    }

    /// Simulate releasing a permit (what dropping an `OwnedSemaphorePermit` does).
    fn release(&self) {
        let mut avail = self.available.lock().unwrap();
        *avail += 1;
    }

    fn available(&self) -> usize {
        *self.available.lock().unwrap()
    }
}

/// Mirrors the real `Controller`'s limit-tracking and resize logic.
struct FakeController {
    permits: Arc<FakePermits>,
    max_permits: usize,
    /// The next limit the algorithm "wants". Set before calling `update`.
    next_limit: usize,
}

impl FakeController {
    fn new(permits: Arc<FakePermits>, initial_limit: usize) -> Self {
        Self {
            permits,
            max_permits: initial_limit,
            next_limit: initial_limit,
        }
    }

    /// Models `Controller::update` — mirrors the real code: algorithm update
    /// (here a no-op, limit is preset) then `resize`.
    fn update(&mut self) {
        self.resize();
    }

    /// Models `Controller::resize`.
    fn resize(&mut self) {
        match self.next_limit.cmp(&self.max_permits) {
            Ordering::Greater => {
                self.permits.add_permits(self.next_limit - self.max_permits);
                self.max_permits = self.next_limit;
            }
            Ordering::Less => {
                let excess = self.max_permits - self.next_limit;
                let forgotten = self.permits.forget_permits(excess);
                self.max_permits -= forgotten;
            }
            Ordering::Equal => {}
        }
    }
}

/// Simulate `FutureGuard::drop`: release the permit, then lock controller and
/// call update.
fn simulate_guard_drop(permits: &FakePermits, controller: &Mutex<FakeController>) {
    // Step 1: release the permit (models `drop(self.permit.take())`)
    permits.release();

    // Step 2: lock and update (models `self.controller.lock().unwrap().update(...)`)
    controller.lock().unwrap().update();
}

/// Test 1: Two threads concurrently drop guards. The controller limit stays the
/// same, so resize is a no-op. We verify no panics, no lost updates, and the
/// final permit count is correct.
#[test]
fn concurrent_guard_drops() {
    loom::model(|| {
        let initial_limit: usize = 4;
        let permits = Arc::new(FakePermits::new(initial_limit));

        // Simulate 2 in-flight requests by pre-decrementing available permits.
        {
            let mut avail = permits.available.lock().unwrap();
            *avail -= 2;
        }

        let controller = Arc::new(Mutex::new(FakeController::new(
            permits.clone(),
            initial_limit,
        )));

        let p1 = permits.clone();
        let c1 = controller.clone();
        let t1 = thread::spawn(move || {
            simulate_guard_drop(&p1, &c1);
        });

        let p2 = permits.clone();
        let c2 = controller.clone();
        let t2 = thread::spawn(move || {
            simulate_guard_drop(&p2, &c2);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Both permits released, no resize happened → available should be back
        // to initial_limit.
        let ctrl = controller.lock().unwrap();
        assert_eq!(permits.available(), ctrl.max_permits);
        assert_eq!(ctrl.max_permits, initial_limit);
    });
}

/// Test 2: One thread drops a guard while another thread's drop triggers a
/// limit decrease (resize-down forgets permits). Verify permit accounting
/// stays consistent.
#[test]
fn concurrent_drop_with_resize_down() {
    loom::model(|| {
        let initial_limit: usize = 4;
        let permits = Arc::new(FakePermits::new(initial_limit));

        // 2 in-flight requests.
        {
            let mut avail = permits.available.lock().unwrap();
            *avail -= 2;
        }

        let controller = Arc::new(Mutex::new(FakeController::new(
            permits.clone(),
            initial_limit,
        )));

        // Thread 1: normal drop, limit stays unchanged.
        let p1 = permits.clone();
        let c1 = controller.clone();
        let t1 = thread::spawn(move || {
            simulate_guard_drop(&p1, &c1);
        });

        // Thread 2: drop triggers a limit decrease to 2.
        let p2 = permits.clone();
        let c2 = controller.clone();
        let t2 = thread::spawn(move || {
            p2.release();
            let mut ctrl = c2.lock().unwrap();
            ctrl.next_limit = 2;
            ctrl.update();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let ctrl = controller.lock().unwrap();
        // After both drops and resize, the permit count should be consistent:
        // available permits should never exceed max_permits.
        assert!(permits.available() <= ctrl.max_permits);
    });
}

/// Test 3: Verify the release-before-lock ordering. When one thread releases a
/// permit before taking the lock, a concurrent thread inside the lock can
/// observe (and forget) that permit via `forget_permits`.
#[test]
fn drop_ordering_permit_released_before_lock() {
    loom::model(|| {
        let initial_limit: usize = 3;
        let permits = Arc::new(FakePermits::new(initial_limit));

        // 2 in-flight requests → 1 available.
        {
            let mut avail = permits.available.lock().unwrap();
            *avail -= 2;
        }

        let controller = Arc::new(Mutex::new(FakeController::new(
            permits.clone(),
            initial_limit,
        )));

        // Thread 1: normal guard drop (release permit, then lock + update with
        // unchanged limit).
        let p1 = permits.clone();
        let c1 = controller.clone();
        let t1 = thread::spawn(move || {
            simulate_guard_drop(&p1, &c1);
        });

        // Thread 2: guard drop that triggers resize-down to 1. The
        // `forget_permits` call inside resize should be able to forget permits
        // released by thread 1 if thread 1's release happened first.
        let p2 = permits.clone();
        let c2 = controller.clone();
        let t2 = thread::spawn(move || {
            p2.release();
            let mut ctrl = c2.lock().unwrap();
            ctrl.next_limit = 1;
            ctrl.update();
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let ctrl = controller.lock().unwrap();
        // Invariant: available permits never exceed max_permits.
        assert!(permits.available() <= ctrl.max_permits);
        // The limit should have been reduced (possibly partially if permits
        // were held).
        assert!(ctrl.max_permits <= initial_limit);
    });
}
