use core::sync::atomic::{AtomicBool, Ordering};

static SCHED_LOCK: AtomicBool = AtomicBool::new(false);

pub(super) fn sched_lock() {
    while SCHED_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

pub(super) fn sched_unlock() {
    SCHED_LOCK.store(false, Ordering::Release);
}
