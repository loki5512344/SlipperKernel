//! Trap dispatch.
use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;
use crate::drivers::plic;
use crate::proc;
use crate::srv::timer;
use crate::syscall::handler;

pub unsafe fn init() {
    crate::arch::csr::write_stvec(crate::arch::asm::trap_entry as *const () as usize as u64);
    let stack_top = &crate::arch::__stack_top as *const u8 as usize;
    crate::arch::csr::write_sscratch(stack_top as u64);
    crate::kinf!(
        "trap",
        "stvec=%p",
        onyx_core::fmt::Arg::from(crate::arch::asm::trap_entry as *const () as usize as u64)
    );
}

pub unsafe fn handle(tf: &mut TrapFrame) {
    let scause = crate::arch::csr::read_scause();
    let is_int = scause & SCAUSE_INT != 0;
    let code = scause & !SCAUSE_INT;
    if is_int {
        match code {
            INTR_S_TIMER => timer::handle(),
            INTR_S_EXTERN => {
                plic::dispatch();
            }
            INTR_S_SOFT => {
                crate::kwrn!("trap", "unhandled S-soft interrupt");
            }
            _ => {
                crate::kwrn!(
                    "trap",
                    "unhandled interrupt: code=%d",
                    onyx_core::fmt::Arg::from(code)
                );
            }
        }
    } else {
        match code {
            CAUSE_U_ECALL => {
                let ret = handler::handle(tf);
                tf.a0 = ret as u64;
                tf.sepc = tf.sepc.wrapping_add(4);
            }
            CAUSE_INST_PF | CAUSE_LD_PF | CAUSE_ST_PF | CAUSE_IAMISS | CAUSE_LDAMISS
            | CAUSE_STAMISS => {
                let pid = proc::current_pid();
                let stval = crate::arch::csr::read_stval();
                let sstatus = crate::arch::csr::read_sstatus();
                let from_kernel = sstatus & SSTATUS_SPP != 0;
                if from_kernel || pid == 0 {
                    crate::kerr!(
                        "trap",
                        "KERNEL page fault sepc=%p stval=%p sstatus=%p",
                        onyx_core::fmt::Arg::from(tf.sepc),
                        onyx_core::fmt::Arg::from(stval),
                        onyx_core::fmt::Arg::from(sstatus)
                    );
                    crate::srv::klog::halt();
                }
                crate::kerr!(
                    "trap",
                    "page fault pid=%d sepc=%p stval=%p",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(tf.sepc),
                    onyx_core::fmt::Arg::from(stval)
                );
                proc::exit(pid, 100 + code as i32);
            }
            CAUSE_ILL => {
                let pid = proc::current_pid();
                crate::kerr!(
                    "trap",
                    "illegal instruction pid=%d sepc=%p",
                    onyx_core::fmt::Arg::from(pid),
                    onyx_core::fmt::Arg::from(tf.sepc)
                );
                proc::exit(pid, 132);
            }
            CAUSE_BRK => {
                let pid = proc::current_pid();
                proc::exit(pid, 133);
            }
            _ => {
                crate::kpanic!(
                    "trap",
                    "unhandled exception: scause=%p sepc=%p",
                    onyx_core::fmt::Arg::from(scause),
                    onyx_core::fmt::Arg::from(tf.sepc)
                );
            }
        }
    }
    // Signal delivery: check the current process for pending unblocked
    // signals. KILL terminates the process; other signals are cleared (MVP).
    proc::signal_check(tf);
    let pid = proc::current_pid();
    if pid != 0 {
        if let Some(p) = proc::by_pid(pid) {
            if matches!(p.state, proc::ProcState::Exited) {
                proc::sched_yield(tf);
                crate::srv::klog::halt();
            }
        }
    }
    if proc::NEED_RESCHED {
        proc::sched_yield(tf);
    }
}
