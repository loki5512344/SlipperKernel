//! TrapFrame — 288 bytes, 36 u64 fields. Must match trap.S offsets.

#![allow(non_snake_case)]
pub const TRAP_FRAME_SIZE: usize = 288;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrapFrame {
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
    pub sepc: u64,
    pub sstatus: u64,
    pub scause: u64,
    pub stval: u64,
    pub satp: u64,
}

impl TrapFrame {
    pub const fn zero() -> Self {
        Self {
            ra: 0,
            sp: 0,
            gp: 0,
            tp: 0,
            t0: 0,
            t1: 0,
            t2: 0,
            s0: 0,
            s1: 0,
            a0: 0,
            a1: 0,
            a2: 0,
            a3: 0,
            a4: 0,
            a5: 0,
            a6: 0,
            a7: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
            t3: 0,
            t4: 0,
            t5: 0,
            t6: 0,
            sepc: 0,
            sstatus: 0,
            scause: 0,
            stval: 0,
            satp: 0,
        }
    }
}

const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TrapFrame>() == TRAP_FRAME_SIZE);
    assert!(offset_of!(TrapFrame, ra) == 0);
    assert!(offset_of!(TrapFrame, sp) == 8);
    assert!(offset_of!(TrapFrame, sepc) == 248);
    assert!(offset_of!(TrapFrame, sstatus) == 256);
    assert!(offset_of!(TrapFrame, satp) == 280);
};
