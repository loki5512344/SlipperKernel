//! PMM — Physical Memory Manager with buddy+SLAB hybrid.
use crate::arch::__kernel_end;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const PAGE_SIZE: usize = 4096;
pub const KERNEL_HEAP_RESERVE: usize = 4 * 1024 * 1024;
const SLAB_SIZES: [usize; 3] = [64, 256, 1024];
const SLAB_MAGIC: u32 = 0x534C_4142;

#[repr(C)]
struct SlabHeader {
    magic: u32,
    size_idx: u32,
    free_bits: u64,
    capacity: u32,
    free_count: u32,
    next: *mut SlabHeader,
}
const fn size_of_slab_header() -> usize {
    32
}

struct Pmm {
    base: usize,
    total_pages: usize,
    free_pages: usize,
    bitmap: *mut u8,
    bitmap_bytes: usize,
    slab_heads: [*mut SlabHeader; SLAB_SIZES.len()],
}

static mut G_PMM: Pmm = Pmm {
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
        bm_set(i);
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

unsafe fn bm_get(bit: usize) -> bool {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) & (1 << (bit % 8)) != 0
}
unsafe fn bm_set(bit: usize) {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) |= 1 << (bit % 8);
    (*&raw mut G_PMM).free_pages -= 1;
}
unsafe fn bm_clr(bit: usize) {
    let p = &raw const G_PMM;
    let bmp = (*p).bitmap;
    *bmp.add(bit / 8) &= !(1 << (bit % 8));
    (*&raw mut G_PMM).free_pages += 1;
}
fn pa_to_idx(pa: usize) -> usize {
    unsafe {
        let p = &raw const G_PMM;
        (pa - (*p).base) / PAGE_SIZE
    }
}
fn idx_to_pa(idx: usize) -> usize {
    unsafe {
        let p = &raw const G_PMM;
        (*p).base + idx * PAGE_SIZE
    }
}

pub unsafe fn alloc() -> KResult<u64> {
    let p = &raw const G_PMM;
    let n = (*p).total_pages;
    let mut i = 0;
    while i < n {
        if !bm_get(i) {
            bm_set(i);
            let pa = idx_to_pa(i);
            ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
            return Ok(pa as u64);
        }
        i += 1;
    }
    Err(Errno::NoMem)
}

pub unsafe fn alloc_n(n: usize) -> KResult<u64> {
    if n == 0 {
        return Err(Errno::Inval);
    }
    let p = &raw const G_PMM;
    let total = (*p).total_pages;
    let mut run = 0usize;
    let mut start = 0usize;
    let mut i = 0;
    while i < total {
        if !bm_get(i) {
            if run == 0 {
                start = i;
            }
            run += 1;
            if run == n {
                for k in start..start + n {
                    bm_set(k);
                }
                return Ok(idx_to_pa(start) as u64);
            }
        } else {
            run = 0;
        }
        i += 1;
    }
    Err(Errno::NoMem)
}

pub unsafe fn free(pa: u64) {
    let idx = pa_to_idx(pa as usize);
    if idx < unsafe { (*&raw const G_PMM).total_pages } {
        if bm_get(idx) {
            bm_clr(idx);
        }
    }
}
pub unsafe fn alloc_zero() -> KResult<u64> {
    alloc()
}
pub fn free_pages() -> usize {
    unsafe { (*&raw const G_PMM).free_pages }
}

unsafe fn slab_class_for(size: usize) -> Option<usize> {
    for (i, &s) in SLAB_SIZES.iter().enumerate() {
        if size <= s {
            return Some(i);
        }
    }
    None
}

pub unsafe fn slab_alloc(size: usize) -> Option<*mut u8> {
    let class = slab_class_for(size)?;
    let obj_size = SLAB_SIZES[class];
    let hdr_size = size_of_slab_header();
    if PAGE_SIZE - hdr_size < obj_size {
        return None;
    }
    let pr = &raw const G_PMM;
    let head = (*pr).slab_heads[class];
    let mut page = head;
    while !page.is_null() {
        let hdr = &mut *page;
        if hdr.free_count > 0 {
            let mut slot = 0u32;
            while slot < hdr.capacity {
                if hdr.free_bits & (1u64 << slot) != 0 {
                    hdr.free_bits &= !(1u64 << slot);
                    hdr.free_count -= 1;
                    return Some((page as usize + hdr_size + slot as usize * obj_size) as *mut u8);
                }
                slot += 1;
            }
        }
        page = hdr.next;
    }
    let new_page_pa = alloc().ok()? as usize;
    let new_page = new_page_pa as *mut SlabHeader;
    let avail = PAGE_SIZE - hdr_size;
    let capacity = (avail / obj_size) as u32;
    let cap64 = capacity as u64;
    let all_free = if cap64 == 64 {
        !0u64
    } else {
        (1u64 << cap64) - 1
    };
    let hdr = &mut *new_page;
    hdr.magic = SLAB_MAGIC;
    hdr.size_idx = class as u32;
    hdr.free_bits = all_free;
    hdr.capacity = capacity;
    hdr.free_count = capacity;
    let pm = &raw const G_PMM;
    hdr.next = (*pm).slab_heads[class];
    (*&raw mut G_PMM).slab_heads[class] = new_page;
    hdr.free_bits &= !1;
    hdr.free_count -= 1;
    Some((new_page as usize + hdr_size) as *mut u8)
}

pub unsafe fn slab_free(ptr: *mut u8) -> bool {
    let page_addr = (ptr as usize) & !(PAGE_SIZE - 1);
    let page = page_addr as *mut SlabHeader;
    if page.is_null() {
        return false;
    }
    let hdr = &mut *page;
    if hdr.magic != SLAB_MAGIC {
        return false;
    }
    let obj_size = SLAB_SIZES[hdr.size_idx as usize];
    let hdr_size = size_of_slab_header();
    let offset = ptr as usize - page_addr - hdr_size;
    if !offset.is_multiple_of(obj_size) {
        return false;
    }
    let slot = (offset / obj_size) as u32;
    if slot >= hdr.capacity {
        return false;
    }
    hdr.free_bits |= 1u64 << slot;
    hdr.free_count += 1;
    if hdr.free_count == hdr.capacity {
        let class = hdr.size_idx as usize;
        let mut cur = (*&raw const G_PMM).slab_heads[class];
        let mut prev: *mut SlabHeader = ptr::null_mut();
        while !cur.is_null() {
            if cur == page {
                if prev.is_null() {
                    (*&raw mut G_PMM).slab_heads[class] = (*cur).next;
                } else {
                    (*prev).next = (*cur).next;
                }
                break;
            }
            prev = cur;
            cur = (*cur).next;
        }
        free(page_addr as u64);
    }
    true
}
