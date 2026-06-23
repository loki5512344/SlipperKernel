#![allow(non_upper_case_globals)]
#![allow(dead_code)]

pub const MSTATUS_MIE: u64 = 1 << 3;
pub const MSTATUS_SIE: u64 = 1 << 1;
pub const MSTATUS_MPP_S: u64 = 1 << 11;
pub const MSTATUS_MPIE: u64 = 1 << 7;
pub const MSTATUS_SPP: u64 = 1 << 8;
pub const SSTATUS_SIE: u64 = 1 << 1;
pub const SSTATUS_SPIE: u64 = 1 << 5;
pub const SSTATUS_SPP: u64 = 1 << 8;
pub const SSTATUS_SUM: u64 = 1 << 18;
pub const SSTATUS_MXR: u64 = 1 << 19;
pub const SATP_MODE_BARE: u64 = 0;
pub const SATP_MODE_SV39: u64 = 8 << 60;
pub const SATP_PPN_MASK: u64 = (1 << 44) - 1;
pub const SCAUSE_INT: u64 = 1 << 63;
pub const CAUSE_IAMISS: u64 = 0;
pub const CAUSE_ILL: u64 = 2;
pub const CAUSE_BRK: u64 = 3;
pub const CAUSE_LDAMISS: u64 = 5;
pub const CAUSE_STAMISS: u64 = 7;
pub const CAUSE_U_ECALL: u64 = 8;
pub const CAUSE_S_ECALL: u64 = 9;
pub const CAUSE_INST_PF: u64 = 12;
pub const CAUSE_LD_PF: u64 = 13;
pub const CAUSE_ST_PF: u64 = 15;
pub const INTR_S_SOFT: u64 = 1;
pub const INTR_S_TIMER: u64 = 5;
pub const INTR_S_EXTERN: u64 = 9;
pub const PTE_V: u64 = 1;
pub const PTE_R: u64 = 2;
pub const PTE_W: u64 = 4;
pub const PTE_X: u64 = 8;
pub const PTE_U: u64 = 16;
pub const PTE_G: u64 = 32;
pub const PTE_A: u64 = 64;
pub const PTE_D: u64 = 128;
pub const PTE_LEAF: u64 = PTE_R | PTE_X;
pub const PTE_PPN_SHIFT: u64 = 10;
pub const PTE_PPN_MASK: u64 = ((1u64 << 44) - 1) << 10;
pub const PTE_FLAGS_MASK: u64 = 0x3FF;
pub const SV39_PTES_PER_TABLE: usize = 512;
#[inline]
pub const fn sv39_l2_idx(va: u64) -> usize {
    ((va >> 30) & 0x1FF) as usize
}
#[inline]
pub const fn sv39_l1_idx(va: u64) -> usize {
    ((va >> 21) & 0x1FF) as usize
}
#[inline]
pub const fn sv39_l0_idx(va: u64) -> usize {
    ((va >> 12) & 0x1FF) as usize
}
pub const CLINT_BASE: u64 = 0x0200_0000;
pub const CLINT_MTIMECMP: u64 = CLINT_BASE + 0x4000;
pub const CLINT_MTIME: u64 = CLINT_BASE + 0xBFF8;
pub const CLINT_FREQ_QEMU: u64 = 10_000_000;
pub const PLIC_BASE: u64 = 0x0C00_0000;
#[inline]
pub const fn plic_smode_ctx(hart: usize) -> usize {
    2 * hart + 1
}
#[inline]
pub const fn plic_priority(irq: u32) -> u64 {
    PLIC_BASE + 4 * irq as u64
}
#[inline]
pub const fn plic_enable(hart: usize, irq: u32) -> u64 {
    PLIC_BASE + 0x2000 + 0x80 * hart as u64 + 4 * (irq as u64 / 32)
}
#[inline]
pub const fn plic_threshold(ctx: usize) -> u64 {
    PLIC_BASE + 0x20_0000 + 0x1000 * ctx as u64
}
#[inline]
pub const fn plic_claim(ctx: usize) -> u64 {
    PLIC_BASE + 0x20_0004 + 0x1000 * ctx as u64
}
#[inline]
pub const fn plic_complete(ctx: usize) -> u64 {
    PLIC_BASE + 0x20_0004 + 0x1000 * ctx as u64
}
pub const KERNEL_BASE: u64 = 0x8020_0000;
pub const USER_BASE: u64 = 0x10000;
pub const USER_TOP: u64 = 0x4000_0000;
pub const USER_STACK_TOP: u64 = USER_TOP - 4096;
pub const USER_HEAP_BASE: u64 = 0x3FF0_0000;
pub const USER_HEAP_SIZE: u64 = 64 * 1024;
pub const USER_STACK_PAGES: usize = 16;
pub const USER_HEAP_PAGES: usize = 16;
pub const ONYXFS_LBA: u32 = 10240;
pub const PLIC_PRIO_UART: u32 = 10;
pub const PLIC_PRIO_VIRTIO: u32 = 1;
