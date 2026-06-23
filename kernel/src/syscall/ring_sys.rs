//! Ring-transition syscalls — `sys_getring` and `sys_dropring`.
use crate::proc;
use onyx_core::errno::Errno;

/// SYS_getring: return current process ring (0/1/2).
pub(super) unsafe fn sys_getring() -> i64 {
    proc::current_ring() as i64
}

/// SYS_dropping: drop to less privileged ring (one-way, never raises).
pub(super) unsafe fn sys_dropring(target: u8) -> i64 {
    let p = proc::current();
    if target < p.ring {
        return Errno::Perm.as_i64();
    } // cannot raise
    if target == p.ring {
        return 0;
    }
    p.ring = target;
    0
}
