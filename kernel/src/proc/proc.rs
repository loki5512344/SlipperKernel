//! Process management with DYNAMIC process allocation (no PROC_MAX limit).
//!
//! Rings: 0=kernel, 1=root space, 2=user space.
//! Processes are heap-allocated nodes in a linked list — no fixed array.
//! Any number of processes can run (limited only by available memory).

use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;
use crate::mm::{heap, vmm};
use crate::proc::onx;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const PROC_RING_KERNEL: u8 = 0;
pub const PROC_RING_ROOT: u8 = 1;
pub const PROC_RING_USER: u8 = 2;

pub const PROC_PID_INIT: u32 = 1;
pub const KSTACK_SIZE: usize = 16 * 1024;

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
    /// Linked list pointer — next process in the global list.
    pub next: *mut Proc,
}

impl Proc {
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
            next: ptr::null_mut(),
        }
    }
}

/// Head of the process linked list.
static mut G_PROC_LIST: *mut Proc = ptr::null_mut();
/// Currently running process (pointer into the list).
static mut G_CURRENT: *mut Proc = ptr::null_mut();
/// Next PID to allocate.
static mut G_NEXT_PID: u32 = PROC_PID_INIT;
/// Need reschedule flag.
pub static mut NEED_RESCHED: bool = false;

pub unsafe fn init() {
    G_PROC_LIST = ptr::null_mut();
    G_CURRENT = ptr::null_mut();
    G_NEXT_PID = PROC_PID_INIT;
}

/// Allocate a new Proc node on the heap and add it to the list.
unsafe fn alloc_proc() -> KResult<*mut Proc> {
    let p = heap::kmalloc(core::mem::size_of::<Proc>())? as *mut Proc;
    // Zero the entire struct.
    ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<Proc>());
    // Initialize fields (kmalloc may not zero — depends on SLAB vs free-list).
    (*p).pid = 0;
    (*p).ring = PROC_RING_KERNEL;
    (*p).state = ProcState::Free;
    (*p).parent_pid = 0;
    (*p).exit_code = 0;
    (*p).root_pa = 0;
    (*p).entry = 0;
    (*p).ustack = 0;
    (*p).heap_brk = 0;
    (*p).uid = 0;
    (*p).gid = 0;
    (*p).tf = TrapFrame::zero();
    (*p).next = G_PROC_LIST;
    G_PROC_LIST = p;
    Ok(p)
}

/// Free a Proc node from the list and heap.
unsafe fn free_proc(p: *mut Proc) {
    // Remove from linked list.
    if G_PROC_LIST == p {
        G_PROC_LIST = (*p).next;
    } else {
        let mut cur = G_PROC_LIST;
        while !cur.is_null() && (*cur).next != p {
            cur = (*cur).next;
        }
        if !cur.is_null() {
            (*cur).next = (*p).next;
        }
    }
    heap::kfree(p as *mut u8);
}

fn alloc_pid() -> u32 {
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

/// Create a user-mode process (ring 1 or 2). Dynamic allocation — no limit.
pub unsafe fn create_user(
    entry: u64,
    ustack: u64,
    root_pa: u64,
    pid: u32,
    parent_pid: u32,
    heap_brk: u64,
    ring: u8,
) -> KResult<()> {
    let p = alloc_proc()?;
    (*p).pid = pid;
    (*p).ring = ring;
    (*p).state = ProcState::Ready;
    (*p).parent_pid = parent_pid;
    (*p).exit_code = 0;
    (*p).root_pa = root_pa;
    (*p).entry = entry;
    (*p).ustack = ustack;
    (*p).heap_brk = heap_brk;
    (*p).uid = if ring <= PROC_RING_ROOT { 0 } else { 1000 };
    (*p).gid = (*p).uid;
    (*p).tf = TrapFrame::zero();
    (*p).tf.sepc = entry;
    (*p).tf.sp = ustack;
    (*p).tf.a0 = 0;
    (*p).tf.a1 = ustack - 256;
    (*p).tf.sstatus = SSTATUS_SPIE;
    (*p).tf.satp = SATP_MODE_SV39 | (root_pa >> 12);
    Ok(())
}

/// **SYS_spawn**: create a new process from .onx file without replacing current.
pub unsafe fn spawn(path: &[u8], ring_hint: u8, parent_pid: u32) -> KResult<u32> {
    use crate::fs::vfs;
    let token = vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK)?;
    let mut size = 0u32;
    vfs::stat(token, &mut size)?;
    if size == 0 {
        vfs::close(token)?;
        return Err(Errno::Inval);
    }
    let img = heap::kmalloc(size as usize)?;
    vfs::read(token, img, size)?;
    vfs::close(token)?;
    let r = onx::load(img, size as usize)?;
    heap::kfree(img);
    let new_pid = alloc_pid();
    let ring = if ring_hint == PROC_RING_ROOT && r.ring == 1 {
        PROC_RING_ROOT
    } else {
        PROC_RING_USER
    };
    create_user(
        r.entry, r.ustack, r.root_pa, new_pid, parent_pid, r.heap_brk, ring,
    )?;
    Ok(new_pid)
}

/// **SYS_wait**: wait for any child to exit. Returns (pid, exit_code).
pub unsafe fn wait(status_out: *mut i32) -> KResult<u32> {
    let my_pid = current_pid();
    // Look for exited child.
    let mut cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
            let exited_pid = (*cur).pid;
            let code = (*cur).exit_code;
            if !status_out.is_null() {
                *status_out = code;
            }
            free_proc(cur);
            return Ok(exited_pid);
        }
        cur = (*cur).next;
    }
    // Check if any child exists.
    let mut has_child = false;
    cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && !matches!((*cur).state, ProcState::Free) {
            has_child = true;
            break;
        }
        cur = (*cur).next;
    }
    if !has_child {
        return Err(Errno::NoEnt);
    }
    Err(Errno::NoEnt) // MVP: no blocking, return immediately.
}

pub unsafe fn enter_user(pid: u32) -> ! {
    // Find process by pid.
    let mut p = G_PROC_LIST;
    while !p.is_null() {
        if (*p).pid == pid && !matches!((*p).state, ProcState::Free) {
            break;
        }
        p = (*p).next;
    }
    if p.is_null() {
        crate::kernel::klog::puts("proc: enter_user: pid not found, halting\n");
        crate::kernel::klog::halt();
    }
    (*p).state = ProcState::Running;
    G_CURRENT = p;
    let entry = (*p).entry as usize;
    let ustack = (*p).ustack as usize;
    let root_pa = (*p).root_pa as usize;
    crate::arch::asm::drop_to_user(entry, ustack, root_pa)
}

pub unsafe fn exit(pid: u32, code: i32) {
    if let Some(p) = by_pid(pid) {
        crate::kerr!(
            "proc",
            "pid %d exited code=%d",
            onyx_core::fmt::Arg::from(pid),
            onyx_core::fmt::Arg::from(code)
        );
        vmm::destroy_root(p.root_pa);
        p.exit_code = code;
        p.state = ProcState::Exited;
        // If parent is waiting, wake it up.
        let parent = p.parent_pid;
        if parent != 0 {
            if let Some(pp) = by_pid(parent) {
                if matches!(pp.state, ProcState::Waiting) {
                    pp.state = ProcState::Ready;
                }
            }
        }
    }
}

pub unsafe fn sched_tick() {
    if !G_CURRENT.is_null() && !matches!((*G_CURRENT).state, ProcState::Free) {
        NEED_RESCHED = true;
    }
}

pub unsafe fn set_need_resched(v: bool) {
    NEED_RESCHED = v;
}

pub unsafe fn sched_yield(tf: &mut TrapFrame) {
    if G_CURRENT.is_null() {
        crate::kernel::klog::puts("proc: no current process, halting\n");
        crate::kernel::klog::halt();
    }
    (*G_CURRENT).tf = *tf;
    if matches!((*G_CURRENT).state, ProcState::Running) {
        (*G_CURRENT).state = ProcState::Ready;
    }
    // Round-robin: find next Ready process after current.
    let _cur_pid = (*G_CURRENT).pid;
    let mut next: *mut Proc = ptr::null_mut();
    // Start from current's next, wrap around.
    let mut start = (*G_CURRENT).next;
    let mut first_pass = true;
    loop {
        let mut cur = start;
        while !cur.is_null() {
            if matches!((*cur).state, ProcState::Ready) {
                next = cur;
                break;
            }
            cur = (*cur).next;
        }
        if !next.is_null() {
            break;
        }
        // Wrap around to head of list.
        if first_pass {
            start = G_PROC_LIST;
            first_pass = false;
        } else {
            break;
        }
    }
    if next.is_null() {
        // No ready process.
        if matches!((*G_CURRENT).state, ProcState::Exited) {
            crate::kernel::klog::puts("proc: no more processes, halting\n");
            crate::kernel::klog::halt();
        }
        (*G_CURRENT).state = ProcState::Running;
        NEED_RESCHED = false;
        *tf = (*G_CURRENT).tf;
        return;
    }
    (*next).state = ProcState::Running;
    G_CURRENT = next;
    NEED_RESCHED = false;
    let next_kstack_top = (*next).kstack.as_ptr().add(KSTACK_SIZE) as usize;
    let dst = (next_kstack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
    ptr::write_volatile(dst, (*next).tf);
    crate::arch::asm::sched_switch(dst as usize);
}

/// Count active processes (for diagnostics).
pub fn count() -> usize {
    unsafe {
        let mut n = 0;
        let mut cur = G_PROC_LIST;
        while !cur.is_null() {
            if !matches!((*cur).state, ProcState::Free) {
                n += 1;
            }
            cur = (*cur).next;
        }
        n
    }
}
