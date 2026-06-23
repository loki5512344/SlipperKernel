//! Assembly: boot.S + trap.S via global_asm!.
//! KEY FIXES vs C-version:
//! - PMP: single 4GB region (pmpaddr0=0x3FFFFFFF, pmpcfg0=0x9F)
//! - trap_entry does NOT switch satp (keeps user satp + SUM bit)
//! - drop_to_user does NOT zero gp/tp
//! - sscratch initialized to __stack_top in trap::init
pub mod boot;
pub mod trap_asm;

pub use trap_asm::{drop_to_user, sched_switch, trap_entry, trap_return};

#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf: *mut crate::arch::trap_frame::TrapFrame) {
    let frame = unsafe { &mut *tf };
    unsafe {
        crate::srv::trap::handle(frame);
    }
}
