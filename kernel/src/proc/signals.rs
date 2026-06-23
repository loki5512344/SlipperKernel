//! Signals (MVP).
//!
//! Signals are delivered via two bitmasks per process: `pending_signals`
//! (delivered but not yet handled) and `signal_mask` (blocked). Signal 9
//! (KILL) is always honored and cannot be blocked. The kernel has no
//! user-space signal handlers in this MVP — KILL terminates the process,
//! every other signal is silently cleared (it serves only as a wakeup
//! mechanism for blocked syscalls like `wait` and `read`).
use crate::arch::trap_frame::TrapFrame;
use onyx_core::errno::{Errno, KResult};

use super::process::{by_pid, exit, ProcState, G_CURRENT};

/// Signal number for KILL (POSIX SIGKILL = 9). Always honored, never blocked.
pub const SIG_KILL: u32 = 9;

/// Deliver `signal` to process `pid`. Sets the corresponding bit in the
/// target's `pending_signals`. If the target is `Waiting`, it is woken
/// (transitioned to `Ready`) so it can run again and observe the signal.
pub unsafe fn signal_send(pid: u32, signal: u32) -> KResult<()> {
    if signal >= 32 {
        return Err(Errno::Inval);
    }
    let p = by_pid(pid).ok_or(Errno::NoEnt)?;
    p.pending_signals |= 1u32 << signal;
    if matches!(p.state, ProcState::Waiting) {
        p.state = ProcState::Ready;
    }
    Ok(())
}

/// Check the current process for pending unblocked signals. Called from the
/// trap handler after every trap (just before returning to user space).
///
/// - Signal 9 (KILL): terminate the process (call `exit` with code 128+9).
///   Sets `NEED_RESCHED` so the trap handler will yield to the next process.
/// - Any other signal: clear its bit (MVP — no user-space handlers).
pub unsafe fn signal_check(tf: &mut TrapFrame) {
    let _ = tf;
    if G_CURRENT.is_null() {
        return;
    }
    let pid = (*G_CURRENT).pid;
    // KILL cannot be blocked — check it first.
    if (*G_CURRENT).pending_signals & (1u32 << SIG_KILL) != 0 {
        (*G_CURRENT).pending_signals &= !(1u32 << SIG_KILL);
        exit(pid, 128 + SIG_KILL as i32);
        super::scheduler::NEED_RESCHED = true;
        return;
    }
    let pending = (*G_CURRENT).pending_signals & !(*G_CURRENT).signal_mask;
    if pending == 0 {
        return;
    }
    // MVP: no user-space handlers — clear all other pending unblocked signals.
    (*G_CURRENT).pending_signals &= (*G_CURRENT).signal_mask;
}
