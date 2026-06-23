//! Scheduler — round-robin cooperative scheduling across processes.
//!
//! `sched_tick` is invoked from the timer interrupt and just sets the
//! `NEED_RESCHED` flag; the actual context switch happens in `sched_yield`,
//! called from the trap handler when `NEED_RESCHED` is set (or explicitly by
//! blocking syscalls). The scheduling policy is simple round-robin over the
//! `G_PROC_LIST` linked list.
use crate::arch::trap_frame::TrapFrame;
use core::ptr;

use super::process::{Proc, ProcState, G_CURRENT, KSTACK_SIZE};

/// Need reschedule flag.
pub static mut NEED_RESCHED: bool = false;

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
        crate::srv::klog::puts("proc: no current process, halting\n");
        crate::srv::klog::halt();
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
            start = super::process::G_PROC_LIST;
            first_pass = false;
        } else {
            break;
        }
    }
    if next.is_null() {
        // No ready process. If the current process is Exited or Waiting, the
        // system is deadlocked (no one can make progress) — halt. Otherwise
        // (current is Running), keep running the current process.
        if matches!((*G_CURRENT).state, ProcState::Exited | ProcState::Waiting) {
            crate::srv::klog::puts("proc: no ready processes, halting\n");
            crate::srv::klog::halt();
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
