#![allow(dead_code)]
#![allow(non_camel_case_types)]

pub type u8 = core::primitive::u8;
pub type u16 = core::primitive::u16;
pub type u32 = core::primitive::u32;
pub type u64 = core::primitive::u64;
pub type usize = core::primitive::usize;
pub type i8 = core::primitive::i8;
pub type i16 = core::primitive::i16;
pub type i32 = core::primitive::i32;
pub type i64 = core::primitive::i64;
pub type isize = core::primitive::isize;
pub type paddr_t = u64;
pub type vaddr_t = u64;
pub type uptr = u64;

pub const KB: usize = 1024;
pub const MB: usize = 1024 * 1024;
pub const GB: usize = 1024 * 1024 * 1024;
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

#[inline]
pub const fn page_align_down(x: usize) -> usize {
    x & PAGE_MASK
}
#[inline]
pub const fn page_align_up(x: usize) -> usize {
    (x + PAGE_SIZE - 1) & PAGE_MASK
}
#[inline]
pub const fn is_aligned(x: usize) -> bool {
    (x & (PAGE_SIZE - 1)) == 0
}
#[inline]
pub const fn min(a: usize, b: usize) -> usize {
    if a < b {
        a
    } else {
        b
    }
}
#[inline]
pub const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}
