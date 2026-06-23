//! CLINT timer (100 Hz tick).
use crate::arch::regs::*;
use crate::arch::{csr, mmio::Mmio};
use crate::proc::proc;
static mut G_MTIME: usize = 0;
static mut G_MTIMECMP: usize = 0;
static mut G_FREQ: u64 = CLINT_FREQ_QEMU;
static mut G_TICK_INTERVAL: u64 = 0;
static mut G_UPTICKS: u64 = 0;
pub static mut G_JIFFIES: u64 = 0;

pub unsafe fn init() {
    let clint = CLINT_BASE;
    G_MTIME = (clint + 0xBFF8) as usize;
    G_MTIMECMP = (clint + 0x4000) as usize;
    G_FREQ = CLINT_FREQ_QEMU;
    G_TICK_INTERVAL = G_FREQ / 100;
    let now = read_mtime();
    write_mtimecmp(now + G_TICK_INTERVAL);
    csr::set_sie(1 << 5);
    crate::kinf!(
        "timer",
        "CLINT @%p, tick=%d ns",
        onyx_core::fmt::Arg::from(clint),
        onyx_core::fmt::Arg::from(1_000_000_000u64 / G_FREQ)
    );
}

unsafe fn read_mtime() -> u64 {
    loop {
        let hi = Mmio::<u32>::at(G_MTIME + 4).read();
        let lo = Mmio::<u32>::at(G_MTIME).read();
        let hi2 = Mmio::<u32>::at(G_MTIME + 4).read();
        if hi == hi2 {
            return ((hi as u64) << 32) | (lo as u64);
        }
    }
}

unsafe fn write_mtimecmp(v: u64) {
    Mmio::<u32>::at(G_MTIMECMP + 4).write(0xFFFF_FFFF);
    Mmio::<u32>::at(G_MTIMECMP).write(v as u32);
    Mmio::<u32>::at(G_MTIMECMP + 4).write((v >> 32) as u32);
}

pub unsafe fn handle() {
    G_UPTICKS = G_UPTICKS.wrapping_add(1);
    G_JIFFIES = G_JIFFIES.wrapping_add(1);
    let now = read_mtime();
    write_mtimecmp(now + G_TICK_INTERVAL);
    proc::sched_tick();
}
pub fn uptime_us() -> u64 {
    unsafe { G_UPTICKS * 10_000 }
}
