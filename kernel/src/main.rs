//! # OnyxKernel — RISC-V 64 (rv64gc) OS with Root Space / User Space isolation
//!
//! Полный порт SlipperKernel→OnyxKernel на Rust.
//! ~98% Rust, assembly через `global_asm!`.
//!
//! ## Rings
//! - 0 (kernel): S-mode, OnyxKernel + drivers
//! - 1 (root space): U-mode, /bin/init + /service/*.bin + /bin/login
//! - 2 (user space): U-mode, /bin/osh + user programs

#![no_std]
#![no_main]
#![warn(clippy::all)]
#![allow(
    clippy::module_inception,
    clippy::missing_safety_doc,
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::match_same_arms,
    clippy::manual_range_contains,
    clippy::manual_memcpy,
    clippy::manual_div_ceil,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::needless_range_loop,
    clippy::unnecessary_wraps,
    clippy::new_without_default,
    clippy::should_implement_trait,
    clippy::not_unsafe_ptr_arg_deref,
    clippy::deref_addrof,
    clippy::collapsible_if,
    dead_code,
    unused_imports,
    unused_parens,
    unsafe_op_in_unsafe_fn,
    static_mut_refs,
    non_upper_case_globals,
    non_snake_case,
    non_camel_case_types
)]

extern crate alloc;
extern crate onyx_core;

pub mod arch;
pub mod drivers;
pub mod fs;
pub mod libfdt;
pub mod mm;
pub mod proc;
pub mod srv;
pub mod syscall;

use core::panic::PanicInfo;

#[no_mangle]
pub unsafe extern "Rust" fn kmain(hartid: usize, fdt_addr: usize) -> ! {
    crate::srv::main::kmain(hartid, fdt_addr)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::srv::klog::panic_handler(info)
}
