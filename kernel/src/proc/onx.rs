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
    //
    // We always OR in PTE_A | PTE_D alongside PTE_U: QEMU's RISC-V machine
    // starts with menvcfg.ADUE = 0, which means the CPU will trap (page
    // fault) on the first access to a leaf PTE whose A bit is clear, instead
    // of setting A/D in hardware. Without these bits every userspace load
    // or store faults on first touch. This matches what vmm::map and
    // vmm::map_anon already do for their own callers.
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

        let seg_flags = (s.flags as u64) | PTE_U | PTE_A | PTE_D;
        let mut va = s.vaddr;
        let end = s.vaddr + s.memsz;
        let mut file_pos: u64 = 0;
        while va < end {
            let page_base = va & !0xFFF;
            let existing = vmm::translate_user(root_pa, page_base);
            let mut page_pa = 0;
            if existing == 0 {
                page_pa = pmm::alloc_zero()?;
                vmm::map_one_pub(root_pa, page_base, page_pa, seg_flags, 0)?;
            }
            let page_end = page_base + 4096;
            let page_va_end = page_end.min(end);
            let copy_len = (page_va_end - va).min(s.filesz.saturating_sub(file_pos));
            if copy_len > 0 {
                let abs_off = s.offset as u64 + file_pos;
                let src = image.add(abs_off as usize);
                let dst = if existing != 0 {
                    (existing as *mut u8).add((va & 0xFFF) as usize)
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
    // Same A/D reasoning as for segment mapping above: QEMU starts with
    // menvcfg.ADUE = 0, so we must set PTE_A | PTE_D ourselves or the very
    // first stack push (e.g. by `drop_to_user` returning to user _start)
    // traps as a store page fault.
    let ustack_top = USER_TOP;
    let ustack_bottom = ustack_top - (USER_STACK_PAGES as u64) * 4096;
    let mut va = ustack_bottom;
    while va < ustack_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D, 0)?;
        va += 4096;
    }

    // User heap.
    let heap_bottom = USER_HEAP_BASE;
    let heap_top = heap_bottom + (USER_HEAP_PAGES as u64) * 4096;
    let mut va = heap_bottom;
    while va < heap_top {
        let page_pa = pmm::alloc_zero()?;
        vmm::map_one_pub(root_pa, va, page_pa, PTE_V | PTE_R | PTE_W | PTE_U | PTE_A | PTE_D, 0)?;
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

const MAX_ARGV: usize = 16;
const MAX_ARGV_BYTES: usize = 1024;

/// Copy argv from parent user space to process address space.
/// Returns (argc, new_sp) where new_sp points to argc value on stack.
unsafe fn argv_ptr_ok(p: u64) -> bool {
    p >= USER_BASE && p < USER_TOP
}

pub(crate) unsafe fn copy_argv_to_stack(root_pa: u64, ustack_top: u64, argv_user: u64) -> (usize, u64) {
    use crate::mm::vmm;
    if argv_user == 0 || !argv_ptr_ok(argv_user) {
        return (0, ustack_top - 16);
    }
    let mut argc = 0usize;
    let mut buf = [0u8; MAX_ARGV_BYTES];
    let mut off = 0usize;
    let ptrs = argv_user as *const u64;
    for i in 0..MAX_ARGV {
        let p = *ptrs.add(i);
        if p == 0 { break; }
        if !argv_ptr_ok(p) { break; }
        let mut slen = 0usize;
        while slen < 127 && *((p + slen as u64) as *const u8) != 0 { slen += 1; }
        if off + slen + 1 > MAX_ARGV_BYTES { break; }
        buf[off..off + slen].copy_from_slice(core::slice::from_raw_parts(p as *const u8, slen));
        off += slen;
        buf[off] = 0;
        off += 1;
        argc += 1;
    }
    if argc == 0 { return (0, ustack_top - 16); }
    let str_size = off;
    let ptr_size = (argc + 1) * 8;
    let total = 8 + ptr_size + str_size;
    let sp = (ustack_top - total as u64) & !15;

    let mut va = sp;
    write_val(root_pa, va, argc as u64); va += 8;
    let str_base = sp + 8 + ptr_size as u64;
    let mut di = 0usize;
    for _ in 0..argc {
        write_val(root_pa, va, str_base + di as u64); va += 8;
        while di < off && buf[di] != 0 { di += 1; }
        di += 1;
    }
    write_val(root_pa, va, 0);

    let dst_pa = vmm::translate(root_pa, sp + 8 + ptr_size as u64);
    if dst_pa != 0 {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst_pa as *mut u8, str_size);
    }
    (argc, sp)
}

unsafe fn write_val(root_pa: u64, va: u64, val: u64) {
    let pa = crate::mm::vmm::translate(root_pa, va);
    if pa != 0 { *(pa as *mut u64) = val; }
}
