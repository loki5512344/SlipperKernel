//! arch — RISC-V 64 архитектурно-зависимый слой.

pub mod asm;
pub mod csr;
pub mod mmio;
pub mod regs;
pub mod smp;
pub mod trap_frame;

pub use regs::*;

extern "Rust" {
    pub static __bss_start: u8;
    pub static __bss_end: u8;
    pub static __stack_top: u8;
    pub static __stack_bottom: u8;
    pub static __kernel_end: u8;
}

#[no_mangle]
pub static SAVED_HARTID: usize = 0;
#[no_mangle]
pub static SAVED_FDT: usize = 0;
