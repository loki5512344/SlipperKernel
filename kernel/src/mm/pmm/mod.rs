//! PMM — Physical Memory Manager with buddy+SLAB hybrid.
//!
//! This is the directory root. It owns the `Pmm` struct, the global `G_PMM`
//! static, the `init` entry point, and the `free_pages` counter. Bitmap
//! operations (alloc/free) live in `bitmap.rs`; SLAB operations live in
//! `slab.rs`.
use crate::arch::__kernel_end;
use core::ptr;

pub const PAGE_SIZE: usize = 4096;
pub const KERNEL_HEAP_RESERVE: usize = 4 * 1024 * 1024;

pub(super) const SLAB_SIZES: [usize; 3] = [64, 256, 1024];
pub(super) const SLAB_MAGIC: u32 = 0x534C_4142;

pub(super) struct Pmm {
    pub(super) base: usize,
    pub(super) total_pages: usize,
    pub(super) free_pages: usize,
    pub(super) bitmap: *mut u8,
    pub(super) bitmap_bytes: usize,
    pub(super) slab_heads: [*mut slab::SlabHeader; SLAB_SIZES.len()],
}

pub(super) static mut G_PMM: Pmm = Pmm {
    base: 0,
    total_pages: 0,
    free_pages: 0,
    bitmap: ptr::null_mut(),
    bitmap_bytes: 0,
    slab_heads: [ptr::null_mut(); SLAB_SIZES.len()],
};

pub unsafe fn init(dram_base: u64, dram_size: u64) {
    let kernel_end_pa = &__kernel_end as *const u8 as usize;
    let heap_end_pa = kernel_end_pa + KERNEL_HEAP_RESERVE;
    let managed_base = core::cmp::max(heap_end_pa, dram_base as usize);
    let managed_base = (managed_base + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let managed_end = (dram_base + dram_size) as usize;
    let managed_size = managed_end.saturating_sub(managed_base);
    let pages = managed_size / PAGE_SIZE;
    let bitmap_bytes = pages.div_ceil(8);
    let bitmap_pages = bitmap_bytes.div_ceil(PAGE_SIZE);
    let bitmap = managed_base as *mut u8;
    ptr::write_bytes(bitmap, 0, bitmap_bytes);
    let data_base = managed_base + bitmap_pages * PAGE_SIZE;
    let data_pages = pages.saturating_sub(bitmap_pages);
    let p = &raw mut G_PMM;
    *p = Pmm {
        base: data_base,
        total_pages: data_pages,
        free_pages: data_pages,
        bitmap,
        bitmap_bytes,
        slab_heads: [ptr::null_mut(); SLAB_SIZES.len()],
    };
    for i in 0..bitmap_pages {
        bitmap::bm_set(i);
    }
    crate::srv::klog::emit(
        crate::srv::klog::Level::Inf,
        "pmm",
        "dram 0x%x + 0x%x, managed base=0x%x pages=%d free=%d",
        &[
            onyx_core::fmt::Arg::from(dram_base),
            onyx_core::fmt::Arg::from(dram_size),
            onyx_core::fmt::Arg::from(data_base as u64),
            onyx_core::fmt::Arg::from(data_pages),
            onyx_core::fmt::Arg::from(data_pages),
        ],
    );
}

pub fn free_pages() -> usize {
    unsafe { (*(&raw const G_PMM)).free_pages }
}

pub fn total_pages() -> usize {
    unsafe { (*(&raw const G_PMM)).total_pages }
}

pub mod bitmap;
pub mod slab;

pub use bitmap::{alloc, alloc_n, alloc_zero, free};
pub use slab::{slab_alloc, slab_free};
