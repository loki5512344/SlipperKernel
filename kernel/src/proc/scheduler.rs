//! Scheduler — round-robin cooperative scheduling across processes.
//!
//! `sched_tick` is invoked from the timer interrupt and just sets the
//! `NEED_RESCHED` flag; the actual context switch happens in `sched_yield`,
//! called from the trap handler when `NEED_RESCHED` is set (or explicitly by
//! blocking syscalls). The scheduling policy is simple round-robin over the
//! `G_PROC_LIST` linked list.
//!
//! SMP: each hart has its own "current" process (`G_HART_CURRENT[hartid]`).
//! A spinlock (`SCHED_LOCK`) protects the run queue from concurrent access.
//! Secondary harts enter the scheduler via `sched_enter_idle()` which sets
//! up trap handling and then parks in a `wfi` loop. When a timer interrupt
//! fires, `sched_yield()` may assign a Ready process to the hart. When the
//! process exits and no replacement exists, the hart returns to idle.
use crate::arch::trap_frame::TrapFrame;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use super::process::{
    current_for_hart, hart_id, set_current_for_hart, Proc, ProcState, G_HART_IDLE_TF,
    G_PROC_LIST, KSTACK_SIZE,
};

/// Need reschedule flag.
pub static mut NEED_RESCHED: bool = false;

/// Scheduler spinlock — protects the process list and G_HART_CURRENT during
/// scheduling decisions. Must be acquired before modifying any shared
/// scheduler state. Released before the context switch (which never returns
/// to the calling frame).
static SCHED_LOCK: AtomicBool = AtomicBool::new(false);

/// Acquire the scheduler spinlock.
fn sched_lock() {
    while SCHED_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

/// Release the scheduler spinlock.
fn sched_unlock() {
    SCHED_LOCK.store(false, Ordering::Release);
}

pub unsafe fn sched_tick() {
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if !cur.is_null() && !matches!((*cur).state, ProcState::Free) {
        NEED_RESCHED = true;
    }
}

pub unsafe fn set_need_resched(v: bool) {
    NEED_RESCHED = v;
}

pub unsafe fn is_idle() -> bool {
    current_for_hart(hart_id()).is_null()
}

pub unsafe fn sched_yield(tf: &mut TrapFrame) {
    let hartid = hart_id();
    let current = current_for_hart(hartid);

    sched_lock();

    if current.is_null() {
        // This hart is idle. Save the idle trap frame so we can restore it
        // later when there is nothing to run.
        G_HART_IDLE_TF[hartid] = *tf;
    } else {
        // Save current process's trap frame.
        (*current).tf = *tf;
        if matches!((*current).state, ProcState::Running) {
            (*current).state = ProcState::Ready;
        }
    }

    // Round-robin: find next Ready process.
    let mut next: *mut Proc = ptr::null_mut();
    let start = if current.is_null() {
        G_PROC_LIST
    } else {
        (*current).next
    };
    let mut first_pass = true;
    let mut cur = start;
    loop {
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
            cur = G_PROC_LIST;
            first_pass = false;
        } else {
            break;
        }
    }

    if next.is_null() {
        // No process to run.
        if current.is_null() {
            // Stay idle — just return, trap handler will resume idle loop.
            sched_unlock();
            NEED_RESCHED = false;
            return;
        }
        if matches!((*current).state, ProcState::Exited) {
            // Current exited and no replacement.
            if hartid == 0 {
                // Primary hart has no idle loop — halt the system.
                set_current_for_hart(hartid, ptr::null_mut());
                sched_unlock();
                NEED_RESCHED = false;
                crate::srv::klog::halt();
            }
            // Secondary hart — switch to idle.
            set_current_for_hart(hartid, ptr::null_mut());
            sched_unlock();
            NEED_RESCHED = false;
            // Switch to the saved idle trap frame on this hart's kernel stack.
            let stack_top = crate::arch::smp::G_SEC_STACKS.as_ptr() as usize
                + (hartid + 1) * crate::arch::smp::SEC_STACK_SIZE;
            let dst =
                (stack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
            ptr::write_volatile(dst, G_HART_IDLE_TF[hartid]);
            crate::arch::asm::sched_switch(dst as usize);
        }
        // Continue running current process.
        (*current).state = ProcState::Running;
        sched_unlock();
        NEED_RESCHED = false;
        return;
    }

    if (*next).tf.sepc == 0 {
        crate::kerr!(
            "sched",
            "pid %d has sepc=0, halting",
            onyx_core::fmt::Arg::from((*next).pid)
        );
        sched_unlock();
        crate::srv::klog::halt();
    }

    (*next).state = ProcState::Running;
    set_current_for_hart(hartid, next);
    sched_unlock();
    NEED_RESCHED = false;

    let next_kstack_top = (*next).kstack.as_ptr().add(KSTACK_SIZE) as usize;
    let dst = (next_kstack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
    ptr::write_volatile(dst, (*next).tf);
    crate::arch::asm::sched_switch(dst as usize);
}

/// Enter the scheduler idle loop on a secondary hart.
///
/// Called from `secondary_kmain()`. Sets up trap handling (stvec, sscratch),
/// initializes the per-hart timer, enables interrupts, and then parks in a
/// `wfi` loop. Timer interrupts will trigger `sched_yield()` which may
/// assign a Ready process to this hart.
pub unsafe fn sched_enter_idle() -> ! {
    let hartid = hart_id();

    // Set up trap handling for this hart.
    crate::arch::csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
    let stack_top = crate::arch::smp::G_SEC_STACKS.as_ptr() as usize
        + (hartid + 1) * crate::arch::smp::SEC_STACK_SIZE;
    crate::arch::csr::write_sscratch(stack_top as u64);

    // Initialize per-hart CLINT timer.
    crate::srv::timer::init_hart(hartid);

    // Enable supervisor timer and external interrupts.
    crate::arch::csr::set_sie((1 << 5) | (1 << 9));

    // Enable global interrupts.
    crate::arch::csr::set_sstatus(crate::arch::regs::SSTATUS_SIE);

    crate::kinf!(
        "sched",
        "hart %d entering idle loop",
        onyx_core::fmt::Arg::from(hartid as u32)
    );

    // Idle loop — wfi until a timer interrupt schedules a process.
    loop {
        crate::arch::csr::wfi();
    }
}
