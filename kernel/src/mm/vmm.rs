//! VMM — Sv39 paging with leaf-splitting.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

static mut G_KERNEL_ROOT_PA: u64 = 0;

pub unsafe fn new_root() -> KResult<u64> {
    pmm::alloc_zero()
}
pub unsafe fn install_root(root_pa: u64) {
    csr::write_satp(SATP_MODE_SV39 | (root_pa >> 12));
    csr::sfence_vma_all();
}

pub unsafe fn init() -> KResult<u64> {
    let root_pa = new_root()?;
    let root = root_pa as *mut u64;
    let leaf_flags = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D;
    for i in 0..3u64 {
        let pa = i << 30;
        ptr::write_volatile(
            root.add(i as usize),
            PTE_V | leaf_flags | (pa >> 12 << PTE_PPN_SHIFT),
        );
    }
    let p = &raw mut G_KERNEL_ROOT_PA;
    *p = root_pa;
    install_root(root_pa);
    Ok(root_pa)
}
pub fn kernel_root() -> u64 {
    unsafe { *(&raw const G_KERNEL_ROOT_PA) }
}

pub unsafe fn destroy_root(root_pa: u64) {
    let root = root_pa as *mut u64;
    free_subtree(root, 2);
    pmm::free(root_pa);
}

unsafe fn free_subtree(table: *mut u64, level: u32) {
    for i in 0..SV39_PTES_PER_TABLE {
        let pte = ptr::read_volatile(table.add(i));
        if pte & PTE_V == 0 {
            continue;
        }
        let is_leaf = pte & PTE_LEAF != 0;
        let child_pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        if is_leaf {
            if pte & PTE_U != 0 {
                pmm::free(child_pa);
            }
        } else if level > 0 {
            free_subtree(child_pa as *mut u64, level - 1);
            pmm::free(child_pa);
        }
    }
}

pub unsafe fn map(root_pa: u64, vaddr: u64, paddr: u64, size: usize, flags: u64) -> KResult<()> {
    let mut va = vaddr;
    let mut pa = paddr;
    let mut remaining = size as u64;
    while remaining > 0 {
        let level = best_level(va, pa, remaining);
        let chunk = if level == 2 {
            1u64 << 30
        } else if level == 1 {
            1u64 << 21
        } else {
            1u64 << 12
        };
        let chunk = chunk.min(remaining);
        map_one(root_pa, va, pa, flags | PTE_A | PTE_D, level)?;
        va += chunk;
        pa += chunk;
        remaining -= chunk;
    }
    Ok(())
}

pub unsafe fn map_anon(root_pa: u64, vaddr: u64, size: usize, flags: u64) -> KResult<()> {
    let mut va = vaddr;
    let mut remaining = size as u64;
    while remaining > 0 {
        let page_pa = pmm::alloc_zero()?;
        map_one(root_pa, va, page_pa, flags | PTE_A | PTE_D, 0)?;
        va += 1u64 << 12;
        remaining -= 1u64 << 12;
    }
    Ok(())
}

pub unsafe fn translate(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=2).rev() {
        let idx = match level {
            2 => sv39_l2_idx(vaddr),
            1 => sv39_l1_idx(vaddr),
            0 => sv39_l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            let leaf_ppn = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
            let off = match level {
                2 => vaddr & ((1u64 << 30) - 1),
                1 => vaddr & ((1u64 << 21) - 1),
                0 => vaddr & ((1u64 << 12) - 1),
                _ => return 0,
            };
            return (leaf_ppn << 12) + off;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

pub unsafe fn translate_user(root_pa: u64, vaddr: u64) -> u64 {
    let mut pa = root_pa;
    for level in (0..=2).rev() {
        let idx = match level {
            2 => sv39_l2_idx(vaddr),
            1 => sv39_l1_idx(vaddr),
            0 => sv39_l0_idx(vaddr),
            _ => return 0,
        };
        let pte = ptr::read_volatile((pa as usize + idx * 8) as *const u64);
        if pte & PTE_V == 0 {
            return 0;
        }
        if pte & PTE_LEAF != 0 {
            if pte & PTE_U == 0 {
                return 0;
            }
            let leaf_ppn = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
            let off = match level {
                2 => vaddr & ((1u64 << 30) - 1),
                1 => vaddr & ((1u64 << 21) - 1),
                0 => vaddr & ((1u64 << 12) - 1),
                _ => return 0,
            };
            return (leaf_ppn << 12) + off;
        }
        pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
    }
    0
}

unsafe fn map_one(root_pa: u64, vaddr: u64, paddr: u64, flags: u64, level: u32) -> KResult<()> {
    let pte_ptr = walk(root_pa, vaddr, level, true)?;
    let pte = PTE_V | flags | ((paddr >> 12) << PTE_PPN_SHIFT);
    ptr::write_volatile(pte_ptr, pte);
    Ok(())
}

pub unsafe fn map_one_pub(
    root_pa: u64,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    level: u32,
) -> KResult<()> {
    map_one(root_pa, vaddr, paddr, flags, level)
}

unsafe fn walk(root_pa: u64, vaddr: u64, leaf_level: u32, create: bool) -> KResult<*mut u64> {
    let mut table_pa = root_pa;
    for level in (leaf_level + 1..=2).rev() {
        let idx = match level {
            2 => sv39_l2_idx(vaddr),
            1 => sv39_l1_idx(vaddr),
            _ => return Err(Errno::Inval),
        };
        let pte_ptr = (table_pa as usize + idx * 8) as *mut u64;
        let pte = ptr::read_volatile(pte_ptr);
        if pte & PTE_V == 0 {
            if !create {
                return Err(Errno::NoEnt);
            }
            let new_pa = pmm::alloc_zero()?;
            ptr::write_volatile(pte_ptr, PTE_V | ((new_pa >> 12) << PTE_PPN_SHIFT));
            table_pa = new_pa;
        } else if pte & PTE_LEAF != 0 {
            if !create {
                return Err(Errno::Inval);
            }
            split_leaf(pte_ptr, pte, level)?;
            let new_pte = ptr::read_volatile(pte_ptr);
            table_pa = (new_pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        } else {
            table_pa = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
        }
    }
    let idx = match leaf_level {
        0 => sv39_l0_idx(vaddr),
        1 => sv39_l1_idx(vaddr),
        2 => sv39_l2_idx(vaddr),
        _ => return Err(Errno::Inval),
    };
    Ok((table_pa as usize + idx * 8) as *mut u64)
}

unsafe fn split_leaf(parent_pte_ptr: *mut u64, parent_pte: u64, parent_level: u32) -> KResult<()> {
    let new_pa = pmm::alloc_zero()?;
    let new_table = new_pa as *mut u64;
    let orig_ppn = (parent_pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT;
    let flags = parent_pte & PTE_FLAGS_MASK;
    let shift = match parent_level {
        2 => 21u32,
        1 => 12u32,
        _ => return Err(Errno::Inval),
    };
    for i in 0..512u64 {
        let sub_pa = (orig_ppn << 12) + i * (1u64 << shift);
        ptr::write_volatile(
            new_table.add(i as usize),
            PTE_V | flags | ((sub_pa >> 12) << PTE_PPN_SHIFT),
        );
    }
    ptr::write_volatile(parent_pte_ptr, PTE_V | ((new_pa >> 12) << PTE_PPN_SHIFT));
    Ok(())
}

fn best_level(va: u64, pa: u64, remaining: u64) -> u32 {
    if remaining >= (1u64 << 30) && (va & ((1u64 << 30) - 1)) == 0 && (pa & ((1u64 << 30) - 1)) == 0
    {
        return 2;
    }
    if remaining >= (1u64 << 21) && (va & ((1u64 << 21) - 1)) == 0 && (pa & ((1u64 << 21) - 1)) == 0
    {
        return 1;
    }
    0
}
