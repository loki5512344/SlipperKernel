//! Syscall subsystem.
//!
//! `abi`     — syscall numbers and ABI constants.
//! `handler` — the central `handle()` entry point, ACL, and `user_ptr_ok`.
//! `fs_sys`  — filesystem syscalls (`sys_write`, `sys_open`, …).
//! `proc_sys`— process syscalls (`sys_exit`, `sys_spawn`, `sys_wait`, …).
//! `snap_sys`— snapshot syscalls (root-only).
//! `ring_sys`— ring-transition syscalls (`sys_getring`, `sys_dropring`).
pub mod abi;
pub mod fs_sys;
pub mod handler;
pub mod proc_sys;
pub mod ring_sys;
pub mod snap_sys;

pub use abi::*;
