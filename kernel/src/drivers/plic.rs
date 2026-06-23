//! PLIC driver — context = 2*hart+1 (S-mode).
use crate::arch::mmio::Mmio;
use crate::arch::regs::*;
static mut G_BASE: u64 = PLIC_BASE;
const HART0_SMODE_CTX: usize = 1;
pub unsafe fn init(base: u64) {
    (*&raw mut G_BASE) = base;
}
pub unsafe fn set_priority(irq: u32, prio: u32) {
    Mmio::<u32>::at(((*&raw const G_BASE) + 4 * irq as u64) as usize).write(prio & 7);
}
pub unsafe fn enable(irq: u32, hart: usize) {
    let ctx = plic_smode_ctx(hart);
    let addr = ((*&raw const G_BASE) + 0x2000 + 0x80 * ctx as u64 + 4 * (irq as u64 / 32)) as usize;
    let m = Mmio::<u32>::at(addr);
    let v = m.read();
    m.write(v | (1u32 << (irq % 32)));
}
pub unsafe fn set_threshold(t: u32) {
    Mmio::<u32>::at(((*&raw const G_BASE) + 0x20_0000 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
        .write(t & 7);
}
pub unsafe fn claim() -> u32 {
    Mmio::<u32>::at(((*&raw const G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
        .read()
}
pub unsafe fn complete(irq: u32) {
    Mmio::<u32>::at(((*&raw const G_BASE) + 0x20_0004 + 0x1000 * HART0_SMODE_CTX as u64) as usize)
        .write(irq);
}
