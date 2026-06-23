//! OnyxExec (.onx) binary loader with ring parsing.
//! Supports both v1 (fixed 8 segments) and v2 (dynamic segments).
use crate::arch::regs::*;
use crate::mm::{pmm, vmm};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::{ONX_FLAGS_RING1, ONX_MAGIC, VMM_R, VMM_W, VMM_X};

pub struct OnxLoadResult {
    pub entry: u64,
    pub root_pa: u64,
    pub ustack: u64,
    pub heap_brk: u64,
    pub ring: u8,
}

pub unsafe fn load(image: *const u8, image_size: usize) -> KResult<OnxLoadResult> {
    if image_size < 24 {
        return Err(Errno::Inval);
    }

    // Parse header using onyx_core::formats.
    let image_slice = core::slice::from_raw_parts(image, image_size);
    let hdr = onyx_core::formats::OnxHeader::from_bytes(image_slice).ok_or(Errno::Inval)?;

    let root_pa = vmm::new_root()?;
    let root = root_pa as *mut u64;

    // 3 baseline 1GB leaves (kernel-only, no U).
    let leaf_flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    for i in 0..3u64 {
        let pa = i << 30;
        ptr::write_volatile(
            root.add(i as usize),
            PTE_V | leaf_flags | (pa >> 12 << PTE_PPN_SHIFT),
        );
    }

    // Map each segment.
    for s in &hdr.segs {
        if s.vaddr < USER_BASE || s.vaddr >= USER_TOP {
            return Err(Errno::Range);
        }
        if s.filesz > s.memsz {
            return Err(Errno::Inval);
        }
        if s.offset as u64 + s.filesz > image_size as u64 {
            return Err(Errno::Range);
        }

        let mut va = s.vaddr;
        let end = s.vaddr + s.memsz;
        let mut file_pos: u64 = 0;
        while va < end {
            let page_pa = pmm::alloc_zero()?;
            let existing = vmm::translate_user(root_pa, va);
            if existing == 0 {
                vmm::map_one_pub(root_pa, va, page_pa, (s.flags as u64) | PTE_U, 0)?;
            }
            let page_va_end = (va + 4096).min(end);
            let copy_len = (page_va_end - va).min(s.filesz.saturating_sub(file_pos));
            if copy_len > 0 {
                let abs_off = s.offset as u64 + file_pos;
                let src = image.add(abs_off as usize);
                let dst = if existing != 0 {
                    existing as *mut u8
                } else {
                    (page_pa as *mut u8).add((va & 0xFFF) as usize)
                };
                ptr::copy_nonoverlapping(src, dst, copy_len as usize);
            }
            file_pos += copy_len;
            va = page_va_end;
        }
    }

    // User stack.
    let ustack_top = USER_TOP;
    let ustack_bottom = ustack_top - (USER_STACK_PAGES as u64) * 4096;
    let mut va = ustack_bottom;
    while va < ustack_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U, 0)?;
        va += 4096;
    }

    // User heap.
    let heap_bottom = USER_HEAP_BASE;
    let heap_top = heap_bottom + (USER_HEAP_PAGES as u64) * 4096;
    let mut va = heap_bottom;
    while va < heap_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U, 0)?;
        va += 4096;
    }

    // Parse ring from header flags.
    let ring = if hdr.flags & ONX_FLAGS_RING1 != 0 {
        1
    } else {
        2
    };

    Ok(OnxLoadResult {
        entry: hdr.entry,
        root_pa,
        ustack: ustack_top - 16,
        heap_brk: heap_bottom,
        ring,
    })
}
