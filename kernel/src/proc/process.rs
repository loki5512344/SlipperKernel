//! Process descriptor, global process-list state, and lookup helpers.
//!
//! Rings: 0=kernel, 1=root space, 2=user space. Processes are heap-allocated
//! nodes in a linked list — no fixed array. Allocation/free, `enter_user`,
//! `exit`, and `count` live in `lifecycle.rs`; spawning/waiting in `spawn.rs`.

use crate::arch::trap_frame::TrapFrame;
use core::ptr;

pub const PROC_RING_KERNEL: u8 = 0;
pub const PROC_RING_ROOT: u8 = 1;
pub const PROC_RING_USER: u8 = 2;

pub const PROC_PID_INIT: u32 = 1;
pub const KSTACK_SIZE: usize = 16 * 1024;
/// Maximum number of open file descriptors per process.
pub const PROC_MAX_FDS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Free = 0,
    Ready = 1,
    Running = 2,
    Exited = 3,
    Waiting = 4,
}

/// Process descriptor. Heap-allocated, linked list.
/// No fixed array — unlimited processes (memory permitting).
#[repr(C, align(16))]
pub struct Proc {
    pub pid: u32,
    pub ring: u8,
    pub state: ProcState,
    pub parent_pid: u32,
    pub exit_code: i32,
    pub root_pa: u64,
    pub entry: u64,
    pub ustack: u64,
    pub heap_brk: u64,
    pub uid: u32,
    pub gid: u32,
    pub tf: TrapFrame,
    pub kstack: [u8; KSTACK_SIZE],
    /// Bitmask of pending (delivered but not yet handled) signals. Bit `s`
    /// indicates signal `s` is pending. Signal 9 (KILL) is always honored.
    pub pending_signals: u32,
    /// Bitmask of blocked signals. Pending signals in this mask are kept
    /// pending until unblocked. Signal 9 (KILL) cannot be blocked.
    pub signal_mask: u32,
    /// Per-process file descriptor table — VFS open files.
    pub fds: [crate::fs::vfs::VfsFd; PROC_MAX_FDS],
    /// Linked list pointer — next process in the global list.
    pub next: *mut Proc,
}

impl Proc {
    #[expect(dead_code)]
    const fn new() -> Self {
        Self {
            pid: 0,
            ring: PROC_RING_KERNEL,
            state: ProcState::Free,
            parent_pid: 0,
            exit_code: 0,
            root_pa: 0,
            entry: 0,
            ustack: 0,
            heap_brk: 0,
            uid: 0,
            gid: 0,
            tf: TrapFrame::zero(),
            kstack: [0; KSTACK_SIZE],
            pending_signals: 0,
            signal_mask: 0,
            fds: [crate::fs::vfs::VfsFd {
                ino: 0,
                size: 0,
                pos: 0,
                fs: crate::fs::vfs::Fs::None,
                used: false,
                perms: 0,
                epoch: 0,
            }; PROC_MAX_FDS],
            next: ptr::null_mut(),
        }
    }
}

/// Head of the process linked list.
pub(super) static mut G_PROC_LIST: *mut Proc = ptr::null_mut();
/// Currently running process (pointer into the list).
pub(super) static mut G_CURRENT: *mut Proc = ptr::null_mut();
/// Next PID to allocate.
pub(super) static mut G_NEXT_PID: u32 = PROC_PID_INIT;

pub unsafe fn init() {
    G_PROC_LIST = ptr::null_mut();
    G_CURRENT = ptr::null_mut();
    G_NEXT_PID = PROC_PID_INIT;
}

pub(super) fn alloc_pid() -> u32 {
    unsafe {
        let pid = G_NEXT_PID;
        G_NEXT_PID = pid + 1;
        pid
    }
}

pub fn current_pid() -> u32 {
    unsafe {
        if G_CURRENT.is_null() {
            return 0;
        }
        let p = &*G_CURRENT;
        if matches!(p.state, ProcState::Running) {
            p.pid
        } else {
            0
        }
    }
}

pub fn current_ring() -> u8 {
    unsafe {
        if G_CURRENT.is_null() {
            return PROC_RING_KERNEL;
        }
        (*G_CURRENT).ring
    }
}

pub unsafe fn current() -> &'static mut Proc {
    &mut *G_CURRENT
}

pub unsafe fn by_pid(pid: u32) -> Option<&'static mut Proc> {
    let mut cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).pid == pid && !matches!((*cur).state, ProcState::Free) {
            return Some(&mut *cur);
        }
        cur = (*cur).next;
    }
    None
}
