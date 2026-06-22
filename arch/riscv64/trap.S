/* SPDX-License-Identifier: GPL-3.0-or-later */
/*
 * SlipperKernel — trap entry/exit assembly.
 *
 * stvec points at trap_entry (direct mode).
 *
 * Layout:
 *   - sscratch holds the kernel sp while in U-mode. csrrw swaps it with
 *     user sp on entry and back on exit.
 *   - trap_frame_t is 288 bytes (36 u64), layout matches include/trap.h.
 */

    .section .text.trap, "ax"
    .balign 4
    .global trap_entry
trap_entry:
    /* Swap sp and sscratch:
     *   before: sp = user_sp, sscratch = kernel_sp
     *   after:  sp = kernel_sp, sscratch = user_sp
     */
    csrrw   sp, sscratch, sp

    /* Allocate trap frame on the kernel stack. */
    addi    sp, sp, -288

    /* ---- Phase 1: save bare minimum using the user page table ---- */

    /* Save the ORIGINAL user t0 BEFORE we clobber t0 with sscratch reads. */
    sd      t0, 32(sp)              /* frame.t0 = user t0 */

    /* Now t0 is free to use as a scratch.  Snapshot user sp from sscratch. */
    csrr    t0, sscratch
    sd      t0, 8(sp)               /* frame.sp = user_sp */

    /* ---- Phase 2: user satp already has kernel identity mappings;
     *     no need to switch page tables.  We set SUM below after saving
     *     registers so S-mode can access U-mode pages. ---- */

    /* All remaining stores use the user page table, which has kernel
     * identity mappings (added by spx_load in spx.c). */
    sd      ra,   0(sp)
    sd      gp,   16(sp)
    sd      tp,   24(sp)
    sd      t1,   40(sp)
    sd      t2,   48(sp)
    sd      s0,   56(sp)
    sd      s1,   64(sp)
    sd      a0,   72(sp)
    sd      a1,   80(sp)
    sd      a2,   88(sp)
    sd      a3,   96(sp)
    sd      a4,   104(sp)
    sd      a5,   112(sp)
    sd      a6,   120(sp)
    sd      a7,   128(sp)
    sd      s2,   136(sp)
    sd      s3,   144(sp)
    sd      s4,   152(sp)
    sd      s5,   160(sp)
    sd      s6,   168(sp)
    sd      s7,   176(sp)
    sd      s8,   184(sp)
    sd      s9,   192(sp)
    sd      s10,  200(sp)
    sd      s11,  208(sp)
    sd      t3,   216(sp)
    sd      t4,   224(sp)
    sd      t5,   232(sp)
    sd      t6,   240(sp)

    /* Set SUM bit so trap handler (S-mode) can access user U-mode pages
     * through the user satp.  Other CSRs are already preserved. */
    li      t0, (1 << 18)
    csrs    sstatus, t0

    /* Save remaining CSRs. */
    csrr    t0, sepc
    sd      t0, 248(sp)
    csrr    t0, sstatus
    sd      t0, 256(sp)

    /* Save satp (user root) — needed for context switch. */
    csrr    t0, satp
    sd      t0, 280(sp)

    /* Call C handler. */
    mv      a0, sp
    call    trap_handler

    /* ------------- Exit path (trap_return) --------------------
     *
     * Goal: restore everything and sret back to U-mode with:
     *   sp    = user_sp   (from frame.sp)
     *   sscratch = kernel_sp (for the next trap)
     *
     * We restore all regs while the kernel page table is still active,
     * then switch to user satp, bump sscratch, and sret in a tight
     * sequence — no interrupt window where sscratch is stale.
     *
     * trap_return is also jumped to from sched_switch after a
     * context switch — sp already points to the target's trap frame.
     */
    .global trap_return
trap_return:
    /* Restore all GPRs from the frame (kernel satp still active). */
    ld      ra,   0(sp)
    ld      gp,   16(sp)
    ld      tp,   24(sp)
    ld      t0,   32(sp)            /* user t0 — properly saved at entry */
    ld      t1,   40(sp)
    ld      t2,   48(sp)
    ld      s0,   56(sp)
    ld      s1,   64(sp)
    ld      a0,   72(sp)
    ld      a1,   80(sp)
    ld      a2,   88(sp)
    ld      a3,   96(sp)
    ld      a4,   104(sp)
    ld      a5,   112(sp)
    ld      a6,   120(sp)
    ld      a7,   128(sp)
    ld      s2,   136(sp)
    ld      s3,   144(sp)
    ld      s4,   152(sp)
    ld      s5,   160(sp)
    ld      s6,   168(sp)
    ld      s7,   176(sp)
    ld      s8,   184(sp)
    ld      s9,   192(sp)
    ld      s10,  200(sp)
    ld      s11,  208(sp)
    ld      t3,   216(sp)
    ld      t4,   224(sp)
    ld      t5,   232(sp)
    ld      t6,   240(sp)

    /* Restore sepc. */
    ld      t0, 248(sp)
    csrw    sepc, t0

    /* Restore sstatus with SIE cleared. The hardware SPIE→SIE
     * transfer on sret will re-enable interrupts in U-mode. */
    ld      t0, 256(sp)
    li      t1, ~(1 << 1)          /* clear SIE */
    and     t0, t0, t1
    csrw    sstatus, t0

    /* Write kernel_sp into sscratch so the next trap has a stack. */
    addi    t1, sp, 288            /* t1 = kernel_sp */
    csrw    sscratch, t1

    /* Pre-load user sp from frame before switching satp. */
    ld      t0, 8(sp)              /* t0 = user_sp */

    /* Restore user satp (switch to user page table).
     * This must happen after all kernel stack accesses. */
    ld      t1, 280(sp)
    csrw    satp, t1
    sfence.vma zero, zero

    /* Switch to user sp and return. */
    mv      sp, t0
    sret

/* sched_switch(new_sp) — called from C to perform a context switch.
 *   a0 = new trap frame address (on the target process's kernel stack)
 *
 * Switches sp to the new trap frame, then jumps into trap_return to
 * restore all registers and sret to the target process in U-mode.
 * Never returns to the caller. */
    .global sched_switch
    .balign 4
sched_switch:
    mv      sp, a0
    j       trap_return

/* void drop_to_user(vaddr_t entry, vaddr_t ustack, paddr_t user_root_pa)
 *   a0 = entry, a1 = ustack, a2 = user root PA
 *
 * Sets up sscratch = (current sp), sstatus (SIE=0, SPP=0, SPIE=1, SUM=1),
 * satp = user_root, sepc = entry, sp = ustack, sret.
 * SIE is cleared so no timer interrupt fires between the satp switch and
 * sret.  SPIE=1 ensures interrupts are re-enabled after sret.
 * Never returns.
 */
    .balign 4
    .global drop_to_user
drop_to_user:
    /* sscratch must hold the kernel sp for the next U->S trap entry. */
    csrw    sscratch, sp

    /* sstatus: clear SIE (disable interrupts during the window below),
     * clear SPP (sret → U-mode), set SPIE (interrupts re-enabled on sret),
     * set SUM (S-mode can access user pages during next trap). */
    csrr    t0, sstatus
    li      t1, ~((1 << 1) | (1 << 8))  /* clear SIE | SPP */
    and     t0, t0, t1
    li      t1, (1 << 5) | (1 << 18)    /* SPIE | SUM */
    or      t0, t0, t1
    csrw    sstatus, t0

    /* Enable supervisor soft (SSI) and external (SEI) interrupts. */
    li      t0, (1 << 1) | (1 << 9)
    csrs    sie, t0

    /* Install user root page table. satp = SV39 | (root_pa >> 12). */
    li      t0, (8UL << 60)
    srli    t1, a2, 12
    or      t0, t0, t1
    csrw    satp, t0
    sfence.vma zero, zero

    /* sepc = entry */
    csrw    sepc, a0
    /* sp = ustack */
    mv      sp, a1

    /* Zero caller-saved regs to avoid leaking kernel state. */
    li      a0, 0
    li      a2, 0
    li      a3, 0
    li      a4, 0
    li      a5, 0
    li      a6, 0
    li      a7, 0
    li      t0, 0
    li      t1, 0
    li      t2, 0
    li      t3, 0
    li      t4, 0
    li      t5, 0
    li      t6, 0
    li      tp, 0
    li      gp, 0
    sret
