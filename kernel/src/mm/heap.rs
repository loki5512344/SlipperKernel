//! Heap allocator (kmalloc/kfree) with SLAB integration.
use crate::mm::pmm;
use onyx_core::errno::{Errno, KResult};
pub const HEAP_SIZE: usize = 4 * 1024 * 1024;
pub const MIN_BLOCK: usize = 16;

#[repr(C)]
struct Block {
    size: usize,
    free: bool,
    next: *mut Block,
    prev: *mut Block,
}
impl Block {
    const fn hdr_size() -> usize {
        core::mem::size_of::<Self>()
    }
}

struct Heap {
    #[expect(dead_code)]
    base: usize,
    #[expect(dead_code)]
    size: usize,
    used: usize,
    free_list: *mut Block,
}
static mut G_HEAP: Heap = Heap {
    base: 0,
    size: 0,
    used: 0,
    free_list: core::ptr::null_mut(),
};

pub unsafe fn init() {
    let kernel_end_pa = &crate::arch::__kernel_end as *const u8 as usize;
    let block = kernel_end_pa as *mut Block;
    (*block).size = HEAP_SIZE - Block::hdr_size();
    (*block).free = true;
    (*block).next = core::ptr::null_mut();
    (*block).prev = core::ptr::null_mut();
    let p = &raw mut G_HEAP;
    *p = Heap {
        base: kernel_end_pa,
        size: HEAP_SIZE,
        used: 0,
        free_list: block,
    };
}

pub unsafe fn kmalloc(size: usize) -> KResult<*mut u8> {
    if size == 0 {
        return Err(Errno::Inval);
    }
    if let Some(p) = pmm::slab_alloc(size) {
        (*&raw mut G_HEAP).used += size;
        return Ok(p);
    }
    let needed = (size + 15) & !15;
    let total = needed + Block::hdr_size();
    let pg = &raw const G_HEAP;
    let mut cur = (*pg).free_list;
    while !cur.is_null() {
        let blk = &mut *cur;
        if blk.free && blk.size >= total {
            if blk.size >= total + MIN_BLOCK + Block::hdr_size() {
                let new_addr = cur as usize + Block::hdr_size() + needed;
                let new_blk = new_addr as *mut Block;
                (*new_blk).size = blk.size - needed - Block::hdr_size();
                (*new_blk).free = true;
                (*new_blk).next = blk.next;
                (*new_blk).prev = cur;
                if !blk.next.is_null() {
                    (*blk.next).prev = new_blk;
                }
                blk.next = new_blk;
                blk.size = needed;
            }
            blk.free = false;
            (*&raw mut G_HEAP).used += needed;
            return Ok((cur as usize + Block::hdr_size()) as *mut u8);
        }
        cur = blk.next;
    }
    Err(Errno::NoMem)
}

pub unsafe fn kfree(p: *mut u8) {
    if p.is_null() {
        return;
    }
    if pmm::slab_free(p) {
        return;
    }
    let blk_addr = p as usize - Block::hdr_size();
    let blk = blk_addr as *mut Block;
    (*blk).free = true;
    if !(*blk).next.is_null() && (*(*blk).next).free {
        let next = (*blk).next;
        (*blk).size += Block::hdr_size() + (*next).size;
        (*blk).next = (*next).next;
        if !(*blk).next.is_null() {
            (*(*blk).next).prev = blk;
        }
    }
    if !(*blk).prev.is_null() && (*(*blk).prev).free {
        let prev = (*blk).prev;
        (*prev).size += Block::hdr_size() + (*blk).size;
        (*prev).next = (*blk).next;
        if !(*prev).next.is_null() {
            (*(*prev).next).prev = prev;
        }
    }
}

pub unsafe fn krealloc(p: *mut u8, new_size: usize) -> KResult<*mut u8> {
    if p.is_null() {
        return kmalloc(new_size);
    }
    if new_size == 0 {
        kfree(p);
        return Err(Errno::Inval);
    }
    let new = kmalloc(new_size)?;
    core::ptr::copy_nonoverlapping(p, new, new_size);
    kfree(p);
    Ok(new)
}
pub fn used() -> usize {
    unsafe { (*&raw const G_HEAP).used }
}
