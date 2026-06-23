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
macro_rules! kdbg { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::kernel::klog::emit($crate::kernel::klog::Level::Dbg, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kinf { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::kernel::klog::emit($crate::kernel::klog::Level::Inf, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kwrn { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::kernel::klog::emit($crate::kernel::klog::Level::Wrn, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kerr { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { $crate::kernel::klog::emit($crate::kernel::klog::Level::Err, $tag, $fmt, &[$($arg),*]) }; }
#[macro_export]
macro_rules! kpanic { ($tag:expr, $fmt:expr $(, $arg:expr)* $(,)?) => { { $crate::kernel::klog::emit($crate::kernel::klog::Level::Err, $tag, $fmt, &[$($arg),*]); $crate::kernel::klog::halt() } }; }

pub use onyx_core::fmt::Arg as FmtArg;

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
