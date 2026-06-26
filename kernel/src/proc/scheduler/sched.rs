use crate::arch::trap_frame::TrapFrame;
use core::ptr;

use super::lock::{sched_lock, sched_unlock};
use crate::proc::process::{
    current_for_hart, hart_id, set_current_for_hart, Proc, ProcState, G_HART_IDLE_TF,
    G_PROC_LIST, KSTACK_SIZE,
};

pub static mut NEED_RESCHED: bool = false;

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

pub unsafe fn sched_yield(tf: &mut TrapFrame) {
    let hartid = hart_id();
    let current = current_for_hart(hartid);

    sched_lock();

    if current.is_null() {
        G_HART_IDLE_TF[hartid] = *tf;
    } else {
        (*current).tf = *tf;
        if matches!((*current).state, ProcState::Running) {
            (*current).state = ProcState::Ready;
        }
    }

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
        if first_pass {
            cur = G_PROC_LIST;
            first_pass = false;
        } else {
            break;
        }
    }

    if next.is_null() {
        if current.is_null() {
            sched_unlock();
            NEED_RESCHED = false;
            return;
        }
        if matches!((*current).state, ProcState::Exited) {
            if hartid == 0 {
                set_current_for_hart(hartid, ptr::null_mut());
                sched_unlock();
                NEED_RESCHED = false;
                crate::srv::klog::halt();
            }
            set_current_for_hart(hartid, ptr::null_mut());
            sched_unlock();
            NEED_RESCHED = false;
            let stack_top = crate::arch::smp::G_SEC_STACKS.as_ptr() as usize
                + (hartid + 1) * crate::arch::smp::SEC_STACK_SIZE;
            let dst =
                (stack_top - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
            ptr::write_volatile(dst, G_HART_IDLE_TF[hartid]);
            crate::arch::asm::sched_switch(dst as usize);
        }
        (*current).state = ProcState::Running;
        sched_unlock();
        NEED_RESCHED = false;
        return;
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
