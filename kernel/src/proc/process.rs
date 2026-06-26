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
    pub mmap_brk: u64,
    pub cwd: [u8; 256],
    pub cwd_len: u16,
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
    /// Wait queue pointer — next process in an IPC wait queue (send/recv).
    pub wait_next: *mut Proc,
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
            mmap_brk: 0x3000_0000,
            cwd: [0; 256],
            cwd_len: 0,
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
            wait_next: ptr::null_mut(),
        }
    }
}

/// Head of the process linked list.
pub(super) static mut G_PROC_LIST: *mut Proc = ptr::null_mut();

pub const MAX_HARTS: usize = crate::arch::smp::MAX_HARTS;

pub(super) static mut G_HART_CURRENT: [*mut Proc; MAX_HARTS] = [ptr::null_mut(); MAX_HARTS];

pub(super) static mut G_HART_IDLE_TF: [TrapFrame; MAX_HARTS] = [TrapFrame::zero(); MAX_HARTS];

pub(super) static mut G_CURRENT: *mut Proc = ptr::null_mut();

pub(super) static mut G_NEXT_PID: u32 = PROC_PID_INIT;

#[inline]
pub fn hart_id() -> usize {
    let id: usize;
    unsafe { core::arch::asm!("mv {0}, tp", out(reg) id) }
    id
}

pub unsafe fn init() {
    G_PROC_LIST = ptr::null_mut();
    G_CURRENT = ptr::null_mut();
    for i in 0..MAX_HARTS {
        G_HART_CURRENT[i] = ptr::null_mut();
    }
    G_NEXT_PID = PROC_PID_INIT;
}

pub(super) fn alloc_pid() -> u32 {
    unsafe {
        let pid = G_NEXT_PID;
        G_NEXT_PID = pid + 1;
        pid
    }
}

pub unsafe fn current_for_hart(hartid: usize) -> *mut Proc {
    if hartid < MAX_HARTS {
        G_HART_CURRENT[hartid]
    } else {
        ptr::null_mut()
    }
}

pub unsafe fn set_current_for_hart(hartid: usize, p: *mut Proc) {
    if hartid < MAX_HARTS {
        G_HART_CURRENT[hartid] = p;
        if hartid == 0 {
            G_CURRENT = p;
        }
    }
}

pub unsafe fn set_cpu_online(hart: usize, v: bool) {
    crate::arch::smp::set_cpu_online(hart, v);
}

pub fn current_pid() -> u32 {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return 0;
        }
        if matches!((*p).state, ProcState::Running) {
            (*p).pid
        } else {
            0
        }
    }
}

pub fn current_ring() -> u8 {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            return PROC_RING_KERNEL;
        }
        (*p).ring
    }
}

pub fn current_opt() -> Option<&'static mut Proc> {
    unsafe {
        let p = G_HART_CURRENT[hart_id()];
        if p.is_null() {
            None
        } else {
            Some(&mut *p)
        }
    }
}

pub unsafe fn current() -> &'static mut Proc {
    let p = G_HART_CURRENT[hart_id()];
    &mut *p
}

pub unsafe fn set_cwd(path: &[u8]) {
    let p = current();
    let n = path.len().min(255);
    p.cwd[..n].copy_from_slice(&path[..n]);
    p.cwd[n] = 0;
    p.cwd_len = n as u16;
}

pub fn cwd() -> &'static [u8] {
    unsafe {
        let p = current();
        &p.cwd[..p.cwd_len as usize]
    }
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

/// Dump all active processes to a `Write` implementor (for kdump).
pub fn dump_all<W: onyx_core::fmt::Write>(w: &mut W) {
    unsafe {
        let mut cur = G_PROC_LIST;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                let state_str = match (*cur).state {
                    ProcState::Ready => "Ready",
                    ProcState::Running => "Running",
                    ProcState::Exited => "Exited",
                    ProcState::Waiting => "Waiting",
                    _ => "???",
                };
                let args: &[onyx_core::fmt::Arg] = &[
                    onyx_core::fmt::Arg::from((*cur).pid),
                    onyx_core::fmt::Arg::from(state_str),
                    onyx_core::fmt::Arg::from((*cur).ring as u32),
                    onyx_core::fmt::Arg::from((*cur).parent_pid),
                ];
                onyx_core::fmt::vformat(w, "    pid=%d state=%s ring=%d ppid=%d\n", args);
            }
            cur = (*cur).next;
        }
    }
}
