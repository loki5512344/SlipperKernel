//! Syscall handler with ACL (ring-aware dispatch).
//!
//! `handle` is the single entry point invoked from the trap handler. It
//! performs the ACL check via `syscall_allowed` and then dispatches to one
//! of the `sys_*` functions defined in the sibling modules
//! (`fs_sys`, `proc_sys`, `snap_sys`, `ring_sys`). User-pointer validation
//! goes through the shared `user_ptr_ok` helper exposed here.
use crate::arch::trap_frame::TrapFrame;
use crate::proc;
use crate::syscall::abi::*;
use onyx_core::errno::Errno;

use super::{fs_sys, proc_sys, ring_sys, snap_sys};

const USER_BASE: u64 = 0x10000;
const USER_TOP: u64 = 0x4000_0000;

pub(super) fn user_ptr_ok(p: u64, len: u64) -> bool {
    p >= USER_BASE && p.checked_add(len).is_some_and(|end| end <= USER_TOP)
}

/// ACL: check if current process ring can call this syscall.
fn syscall_allowed(nr: u64, ring: u8) -> bool {
    match nr {
        // Available to all (ring 2 = user):
        SYS_write | SYS_read | SYS_exit | SYS_yield | SYS_getpid | SYS_sbrk | SYS_open
        | SYS_close | SYS_lseek | SYS_stat | SYS_exec | SYS_readdir | SYS_getring
        | SYS_dropring | SYS_sigmask | SYS_write_fd => true,
        // Root-only (ring 0 or 1):
        SYS_spawn
        | SYS_wait
        | SYS_snapshot_create
        | SYS_snapshot_rollback
        | SYS_snapshot_list
        | SYS_kill
        | SYS_create
        | SYS_mkdir => ring <= proc::PROC_RING_ROOT,
        // Stubbed:
        SYS_brk | SYS_mmap => false,
        _ => false,
    }
}

pub unsafe fn handle(tf: &mut TrapFrame) -> i64 {
    let nr = tf.a7;
    let a0 = tf.a0;
    let a1 = tf.a1;
    let a2 = tf.a2;

    let cur_ring = proc::current_ring();

    // ACL check.
    if !syscall_allowed(nr, cur_ring) {
        return Errno::Perm.as_i64();
    }

    match nr {
        SYS_write => fs_sys::sys_write(tf, a0, a1, a2),
        SYS_read => fs_sys::sys_read(tf, a0, a1, a2),
        SYS_exit => proc_sys::sys_exit(a0),
        SYS_yield => proc_sys::sys_yield(),
        SYS_getpid => proc_sys::sys_getpid(),
        SYS_open => fs_sys::sys_open(a0, a1, a2),
        SYS_close => fs_sys::sys_close(a0),
        SYS_lseek => fs_sys::sys_lseek(a0, a1 as i64, a2 as u32),
        SYS_stat => fs_sys::sys_stat(a0, a1),
        SYS_exec => fs_sys::sys_exec(tf, a0),
        SYS_sbrk => fs_sys::sys_sbrk(a0 as i64),
        SYS_spawn => proc_sys::sys_spawn(a0, a1 as u8),
        SYS_wait => proc_sys::sys_wait(tf, a0),
        SYS_readdir => fs_sys::sys_readdir(a0, a1, a2),
        SYS_getring => ring_sys::sys_getring(),
        SYS_dropring => ring_sys::sys_dropring(a0 as u8),
        SYS_snapshot_create => snap_sys::sys_snapshot_create(a0),
        SYS_snapshot_rollback => snap_sys::sys_snapshot_rollback(a0 as u32),
        SYS_snapshot_list => snap_sys::sys_snapshot_list(a0, a1),
        SYS_kill => proc_sys::sys_kill(a0 as u32, a1 as u32),
        SYS_sigmask => proc_sys::sys_sigmask(a0 as u32, a1 as u32),
        SYS_write_fd => fs_sys::sys_write_fd(a0, a1, a2),
        SYS_create => fs_sys::sys_create(a0, a1, a2),
        SYS_mkdir => fs_sys::sys_mkdir(a0),
        SYS_brk | SYS_mmap => Errno::NoSys.as_i64(),
        _ => Errno::NoSys.as_i64(),
    }
}
