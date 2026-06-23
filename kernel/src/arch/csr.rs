//! CSR access via inline asm.
use core::arch::asm;

#[inline]
pub unsafe fn read_sstatus() -> u64 {
    let v: u64;
    asm!("csrr {0}, sstatus", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn write_sstatus(v: u64) {
    asm!("csrw sstatus, {0}", in(reg) v, options(nomem, nostack));
}
#[inline]
pub unsafe fn set_sstatus(m: u64) {
    asm!("csrs sstatus, {0}", in(reg) m, options(nomem, nostack));
}
#[inline]
pub unsafe fn clear_sstatus(m: u64) {
    asm!("csrc sstatus, {0}", in(reg) m, options(nomem, nostack));
}
#[inline]
pub unsafe fn read_sepc() -> u64 {
    let v: u64;
    asm!("csrr {0}, sepc", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn write_sepc(v: u64) {
    asm!("csrw sepc, {0}", in(reg) v, options(nomem, nostack));
}
#[inline]
pub unsafe fn read_scause() -> u64 {
    let v: u64;
    asm!("csrr {0}, scause", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn read_stval() -> u64 {
    let v: u64;
    asm!("csrr {0}, stval", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn read_satp() -> u64 {
    let v: u64;
    asm!("csrr {0}, satp", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn write_satp(v: u64) {
    asm!("csrw satp, {0}", in(reg) v, options(nomem, nostack));
}
#[inline]
pub unsafe fn write_stvec(v: u64) {
    asm!("csrw stvec, {0}", in(reg) v, options(nomem, nostack));
}
#[inline]
pub unsafe fn read_sie() -> u64 {
    let v: u64;
    asm!("csrr {0}, sie", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn set_sie(m: u64) {
    asm!("csrs sie, {0}", in(reg) m, options(nomem, nostack));
}
#[inline]
pub unsafe fn clear_sie(m: u64) {
    asm!("csrc sie, {0}", in(reg) m, options(nomem, nostack));
}
#[inline]
pub unsafe fn write_sscratch(v: u64) {
    asm!("csrw sscratch, {0}", in(reg) v, options(nomem, nostack));
}
#[inline]
pub unsafe fn read_mhartid() -> u64 {
    let v: u64;
    asm!("csrr {0}, mhartid", out(reg) v, options(nomem, nostack));
    v
}
#[inline]
pub unsafe fn sfence_vma_all() {
    asm!("sfence.vma zero, zero", options(nostack));
}
#[inline]
pub unsafe fn sfence_vma(va: u64, asid: u64) {
    asm!("sfence.vma {0}, {1}", in(reg) va, in(reg) asid, options(nostack));
}
#[inline]
pub unsafe fn wfi() {
    asm!("wfi", options(nostack));
}
