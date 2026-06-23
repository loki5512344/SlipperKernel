//! Assembly: boot.S + trap.S via global_asm!.
//! KEY FIXES vs C-version:
//! - PMP: single 4GB region (pmpaddr0=0x3FFFFFFF, pmpcfg0=0x9F)
//! - trap_entry does NOT switch satp (keeps user satp + SUM bit)
//! - drop_to_user does NOT zero gp/tp
//! - sscratch initialized to __stack_top in trap::init

use super::{__bss_end, __bss_start, __stack_top, SAVED_FDT, SAVED_HARTID};
use core::arch::global_asm;

// ─── boot.S ──────────────────────────────────────────────────────────────────
global_asm!(
    r#"
.section .text.boot
.global _start
_start:
    bnez a0, park
    la t0, {saved_hartid}
    sd a0, 0(t0)
    la t0, {saved_fdt}
    sd a1, 0(t0)
    la t0, {bss_start}
    la t1, {bss_end}
1:  bgeu t0, t1, 2f
    sd zero, 0(t0)
    addi t0, t0, 8
    j 1b
2:
    la sp, {stack_top}
    li t0, 0x3FFFFFFF
    csrw pmpaddr0, t0
    li t0, 0x9F
    csrw pmpcfg0, t0
    li t0, (1<<0)|(1<<2)|(1<<3)|(1<<5)|(1<<7)|(1<<8)|(1<<9)|(1<<12)|(1<<13)|(1<<15)
    csrw medeleg, t0
    li t0, (1<<1)|(1<<5)|(1<<9)
    csrw mideleg, t0
    csrw mie, zero
    li t0, (1<<11)
    csrs mstatus, t0
    li t0, (1<<7)
    csrc mstatus, t0
    la t0, kmain
    csrw mepc, t0
    la t0, {saved_hartid}
    ld a0, 0(t0)
    la t0, {saved_fdt}
    ld a1, 0(t0)
    csrw satp, zero
    mret
park:
    wfi
    j park
"#,
    saved_hartid = sym SAVED_HARTID,
    saved_fdt = sym SAVED_FDT,
    bss_start = sym __bss_start,
    bss_end = sym __bss_end,
    stack_top = sym __stack_top,
);

// ─── trap.S ──────────────────────────────────────────────────────────────────
global_asm!(
    r#"
.section .text.trap
.balign 4
.global trap_entry
trap_entry:
    csrrw sp, sscratch, sp
    addi sp, sp, -288
    sd t0, 32(sp)
    csrr t0, sscratch
    sd t0, 8(sp)
    sd ra, 0(sp)
    sd gp, 16(sp)
    sd tp, 24(sp)
    sd t1, 40(sp)
    sd t2, 48(sp)
    sd s0, 56(sp)
    sd s1, 64(sp)
    sd a0, 72(sp)
    sd a1, 80(sp)
    sd a2, 88(sp)
    sd a3, 96(sp)
    sd a4, 104(sp)
    sd a5, 112(sp)
    sd a6, 120(sp)
    sd a7, 128(sp)
    sd s2, 136(sp)
    sd s3, 144(sp)
    sd s4, 152(sp)
    sd s5, 160(sp)
    sd s6, 168(sp)
    sd s7, 176(sp)
    sd s8, 184(sp)
    sd s9, 192(sp)
    sd s10, 200(sp)
    sd s11, 208(sp)
    sd t3, 216(sp)
    sd t4, 224(sp)
    sd t5, 232(sp)
    sd t6, 240(sp)
    li t0, (1 << 18)
    csrs sstatus, t0
    csrr t0, sepc
    sd t0, 248(sp)
    csrr t0, sstatus
    sd t0, 256(sp)
    csrr t0, satp
    sd t0, 280(sp)
    mv a0, sp
    call trap_handler

.global trap_return
trap_return:
    ld ra, 0(sp)
    ld gp, 16(sp)
    ld tp, 24(sp)
    ld t0, 32(sp)
    ld t1, 40(sp)
    ld t2, 48(sp)
    ld s0, 56(sp)
    ld s1, 64(sp)
    ld a0, 72(sp)
    ld a1, 80(sp)
    ld a2, 88(sp)
    ld a3, 96(sp)
    ld a4, 104(sp)
    ld a5, 112(sp)
    ld a6, 120(sp)
    ld a7, 128(sp)
    ld s2, 136(sp)
    ld s3, 144(sp)
    ld s4, 152(sp)
    ld s5, 160(sp)
    ld s6, 168(sp)
    ld s7, 176(sp)
    ld s8, 184(sp)
    ld s9, 192(sp)
    ld s10, 200(sp)
    ld s11, 208(sp)
    ld t3, 216(sp)
    ld t4, 224(sp)
    ld t5, 232(sp)
    ld t6, 240(sp)
    ld t0, 248(sp)
    csrw sepc, t0
    ld t0, 256(sp)
    li t1, ~(1 << 1)
    and t0, t0, t1
    csrw sstatus, t0
    addi t1, sp, 288
    csrw sscratch, t1
    ld t0, 8(sp)
    ld t1, 280(sp)
    csrw satp, t1
    sfence.vma zero, zero
    mv sp, t0
    sret

.global sched_switch
sched_switch:
    mv sp, a0
    j trap_return

.global drop_to_user
drop_to_user:
    csrw sscratch, sp
    li t0, (1 << 1) | (1 << 8)
    csrc sstatus, t0
    li t0, (1 << 5) | (1 << 18)
    csrs sstatus, t0
    li t0, (1 << 1) | (1 << 9)
    csrs sie, t0
    li t0, (8 << 60)
    srli t1, a2, 12
    or t0, t0, t1
    csrw satp, t0
    sfence.vma zero, zero
    csrw sepc, a0
    mv sp, a1
    li a0, 0
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a5, 0
    li a6, 0
    li a7, 0
    li t0, 0
    li t1, 0
    li t2, 0
    li t3, 0
    li t4, 0
    li t5, 0
    li t6, 0
    sret
"#,
);

extern "Rust" {
    pub fn trap_entry();
    pub fn trap_return();
    pub fn sched_switch(new_sp: usize) -> !;
    pub fn drop_to_user(entry: usize, ustack: usize, user_root_pa: usize) -> !;
}

#[no_mangle]
pub extern "C" fn trap_handler(tf: *mut crate::arch::trap_frame::TrapFrame) {
    let frame = unsafe { &mut *tf };
    unsafe {
        crate::srv::trap::handle(frame);
    }
}
