//! klog — formatted logging via UART.
use crate::drivers::uart;
use core::panic::PanicInfo;
use onyx_core::fmt::{vformat, Arg, Write};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Err = 0,
    Wrn = 1,
    Inf = 2,
    Dbg = 3,
}
impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dbg => "DBG",
            Self::Inf => "INF",
            Self::Wrn => "WRN",
            Self::Err => "ERR",
        }
    }
}

pub const KLOG_LEVEL: u8 = 3; // Dbg (print all)

struct UartWriter;
impl Write for UartWriter {
    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if b == b'\n' {
                uart::putc(b'\r');
            }
            uart::putc(b);
        }
    }
    fn write_char(&mut self, c: u8) {
        if c == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(c);
    }
}

pub fn puts(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            uart::putc(b'\r');
        }
        uart::putc(b);
    }
}
pub fn putc(c: u8) {
    if c == b'\n' {
        uart::putc(b'\r');
    }
    uart::putc(c);
}

pub fn emit(level: Level, tag: &str, fmt: &str, args: &[Arg]) {
    if (level as u8) > KLOG_LEVEL {
        return;
    }
    let mut w = UartWriter;
    w.write_char(b'[');
    w.write_str(level.as_str());
    w.write_char(b']');
    w.write_char(b' ');
    w.write_str(tag);
    w.write_str(": ");
    vformat(&mut w, fmt, args);
    w.write_char(b'\n');
}

#[macro_export]
macro_rules! kdbg { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::srv::klog::emit($crate::srv::klog::Level::Dbg, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kinf { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::srv::klog::emit($crate::srv::klog::Level::Inf, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kwrn { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::srv::klog::emit($crate::srv::klog::Level::Wrn, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kerr { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kpanic { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { { $crate::srv::klog::emit($crate::srv::klog::Level::Err, $tag, $fmt, &[$($arg),*]); $crate::srv::klog::halt() } }; }

pub use onyx_core::fmt::Arg as FmtArg;

/// Simple delay loop (approximate, for panic reboot).
fn delay_loops(n: u64) {
    for _ in 0..n {
        unsafe { core::arch::asm!("nop") }
    }
}

pub fn panic_handler(info: &PanicInfo) -> ! {
    unsafe {
        crate::arch::csr::clear_sstatus(crate::arch::regs::SSTATUS_SIE);
    }
    let mut w = UartWriter;
    w.write_str("\n\n*** KERNEL PANIC ***\n");
    if let Some(loc) = info.location() {
        let args: &[Arg] = &[
            Arg::from(loc.file()),
            Arg::from(loc.line()),
            Arg::from(loc.column()),
        ];
        vformat(&mut w, "  at %s:%d:%d\n", args);
    }
    #[allow(deprecated)]
    if let Some(msg) = info.payload().downcast_ref::<&str>() {
        w.write_str("  msg: ");
        w.write_str(msg);
        w.write_char(b'\n');
    }
    // kdump: dump CSR state
    unsafe {
        let sepc = crate::arch::csr::read_sepc();
        let sstatus = crate::arch::csr::read_sstatus();
        let scause = crate::arch::csr::read_scause();
        let stval = crate::arch::csr::read_stval();
        let args: &[Arg] = &[
            Arg::from(sepc),
            Arg::from(sstatus),
            Arg::from(scause),
            Arg::from(stval),
        ];
        vformat(&mut w, "  sepc=%p sstatus=%p scause=%p stval=%p\n", args);
    }

    // kdump: stack trace (frame pointer walk)
    w.write_str("\n  Stack trace:\n");
    let mut fp: usize;
    unsafe { core::arch::asm!("mv {0}, fp", out(reg) fp) }
    for depth in 0..16 {
        if fp == 0 || fp < 0x8000_0000 {
            break;
        }
        unsafe {
            let ra = *((fp + 8) as *const usize);
            let args: &[Arg] = &[Arg::from(depth as u64), Arg::from(ra as u64)];
            vformat(&mut w, "    #%d: %p\n", args);
            fp = *(fp as *const usize);
        }
    }

    // kdump: dump current process
    let pid = crate::proc::current_pid();
    if pid != 0 {
        let args: &[Arg] = &[Arg::from(pid)];
        vformat(&mut w, "  pid=%d\n", args);
    }

    // kdump: dump process count
    let cnt = crate::proc::count();
    let args: &[Arg] = &[Arg::from(cnt)];
    vformat(&mut w, "  processes=%d\n", args);

    // kdump: dump all active processes
    w.write_str("\n  Active processes:\n");
    crate::proc::dump_all(&mut w);

    // Reboot via QEMU virt test finisher device
    w.write_str("\n  Rebooting in 3 seconds...\n");
    delay_loops(300_000_000);
    // QEMU virt test finisher: write 0x5555 to 0x100000 for reboot
    unsafe {
        let finisher = 0x100000usize as *mut u32;
        core::ptr::write_volatile(finisher, 0x5555);
    }
    // If not QEMU (write had no effect), just halt
    halt();
}

pub fn halt() -> ! {
    unsafe {
        crate::arch::csr::clear_sstatus(crate::arch::regs::SSTATUS_SIE);
        loop {
            crate::arch::csr::wfi();
        }
    }
}
