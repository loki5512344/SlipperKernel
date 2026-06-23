//! Process lifecycle — allocation, freeing, `enter_user`, `exit`, and `count`.
use super::process::Proc;
use super::process::{by_pid, ProcState, G_CURRENT, G_PROC_LIST, PROC_RING_KERNEL};
use crate::arch::trap_frame::TrapFrame;
use crate::mm::{heap, vmm};
use core::ptr;
use onyx_core::errno::KResult;

/// Allocate a new Proc node on the heap and add it to the list.
pub(super) unsafe fn alloc_proc() -> KResult<*mut Proc> {
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
    (*p).pending_signals = 0;
    (*p).signal_mask = 0;
    // Initialize per-process FD table — all slots free.
    for fd in (*p).fds.iter_mut() {
        *fd = crate::fs::vfs::VfsFd::default();
    }
    (*p).next = G_PROC_LIST;
    G_PROC_LIST = p;
    Ok(p)
}

/// Free a Proc node from the list and heap.
pub(super) unsafe fn free_proc(p: *mut Proc) {
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
        crate::srv::klog::puts("proc: enter_user: pid not found, halting\n");
        crate::srv::klog::halt();
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
