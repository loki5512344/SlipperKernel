//! Process-management syscalls — `sys_exit`, `sys_yield`, `sys_getpid`,
//! `sys_spawn`, `sys_wait`, `sys_kill`, `sys_sigmask`.
use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use onyx_core::errno::Errno;

use super::handler::user_ptr_ok;

pub(super) unsafe fn sys_exit(code: u64) -> i64 {
    let pid = proc::current_pid();
    proc::exit(pid, code as i32);
    0
}

pub(super) unsafe fn sys_yield() -> i64 {
    proc::set_need_resched(true);
    0
}
pub(super) unsafe fn sys_getpid() -> i64 {
    proc::current_pid() as i64
}

/// SYS_spawn: create new process from .onx file.
pub(super) unsafe fn sys_spawn(_tf: &mut TrapFrame, path: u64, argv: u64, ring_hint: u8) -> i64 {
    if !user_ptr_ok(path, 1) {
        return Errno::Inval.as_i64();
    }
    let mut len = 0usize;
    let p = path as *const u8;
    while *p.add(len) != 0 && len < 256 {
        len += 1;
    }
    let path_bytes = core::slice::from_raw_parts(p, len);
    let parent_pid = proc::current_pid();
    match proc::spawn(path_bytes, argv, ring_hint, parent_pid) {
        Ok(pid) => pid as i64,
        Err(e) => e.as_i64(),
    }
}

/// SYS_wait: wait for child exit. Blocks (yields) until a child exits.
pub(super) unsafe fn sys_wait(tf: &mut TrapFrame, status_out: u64) -> i64 {
    let status_ptr = if status_out != 0 && user_ptr_ok(status_out, 4) {
        status_out as *mut i32
    } else {
        core::ptr::null_mut()
    };
    match proc::wait(tf, status_ptr) {
        Ok(pid) => pid as i64,
        Err(e) => e.as_i64(),
    }
}

// ── Signal syscalls ───────────────────────────────────────────────────────

/// SYS_kill(pid, signal): deliver `signal` to process `pid`.
/// Root-only (ACL enforced in `syscall_allowed`).
pub(super) unsafe fn sys_kill(pid: u32, signal: u32) -> i64 {
    match proc::signal_send(pid, signal) {
        Ok(()) => 0,
        Err(e) => e.as_i64(),
    }
}

/// SYS_sigmask(how, sig): block / unblock / set the signal mask for one
/// signal. `how`: 0 = block, 1 = unblock, 2 = set mask to just `sig`.
/// Signal 9 (KILL) cannot be blocked — `how == 0` on signal 9 is a no-op.
pub(super) unsafe fn sys_sigmask(how: u32, sig: u32) -> i64 {
    if sig >= 32 {
        return Errno::Inval.as_i64();
    }
    let p = proc::current();
    match how {
        0 => {
            // Block — but KILL cannot be blocked.
            if sig != proc::SIG_KILL {
                p.signal_mask |= 1u32 << sig;
            }
        }
        1 => {
            p.signal_mask &= !(1u32 << sig);
        }
        2 => {
            // Set mask to exactly {sig} (plus KILL-ignoring: KILL still
            // cannot be blocked, so don't add it).
            let mut m = 0u32;
            if sig != proc::SIG_KILL {
                m = 1u32 << sig;
            }
            p.signal_mask = m;
        }
        _ => return Errno::Inval.as_i64(),
    }
    0
}
