//! MMIO register accessor.

use core::ptr::{read_volatile, write_volatile};
pub trait MmioReg: Copy + Sized {}
impl MmioReg for u8 {}
impl MmioReg for u16 {}
impl MmioReg for u32 {}
impl MmioReg for u64 {}

#[derive(Clone, Copy)]
pub struct Mmio<T: MmioReg> {
    addr: usize,
    _p: core::marker::PhantomData<T>,
}
impl<T: MmioReg> Mmio<T> {
    #[inline]
    pub const fn at(addr: usize) -> Self {
        Self {
            addr,
            _p: core::marker::PhantomData,
        }
    }
    #[inline]
    pub unsafe fn read(self) -> T {
        read_volatile(self.addr as *const T)
    }
    #[inline]
    pub unsafe fn write(self, v: T) {
        write_volatile(self.addr as *mut T, v);
    }
    #[inline]
    pub const fn addr(self) -> usize {
        self.addr
    }
}

#[derive(Clone, Copy)]
pub struct MmioBlock {
    base: usize,
    shift: u32,
}
impl MmioBlock {
    pub const fn new(base: usize, shift: u32) -> Self {
        Self { base, shift }
    }
    #[inline]
    pub const fn reg_u8(self, off: u32) -> Mmio<u8> {
        Mmio::at(self.base + (off << self.shift) as usize)
    }
    #[inline]
    pub const fn reg_u32(self, off: u32) -> Mmio<u32> {
        Mmio::at(self.base + (off << self.shift) as usize)
    }
}
