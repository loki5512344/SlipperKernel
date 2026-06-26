//! VMM — Sv39 paging with leaf-splitting.
//!
//! This is the directory root. It owns the kernel root page-table pointer
//! (`G_KERNEL_ROOT_PA`), the `new_root`/`install_root`/`init`/`kernel_root`
//! lifecycle helpers, `destroy_root` (with `free_subtree`), and the
//! `translate`/`translate_user` walkers. Map operations live in `map.rs`;
//! the page-table walker and leaf-splitting live in `walk.rs`.
use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

pub(super) static mut G_KERNEL_ROOT_PA: u64 = 0;

pub unsafe fn new_root() -> KResult<u64> {
    pmm::alloc_zero()
}

pub unsafe fn install_root(root_pa: u64) {
    csr::write_satp(SATP_MODE_SV39 | (root_pa >> 12));
    csr::sfence_vma_all();
}

pub unsafe fn init() -> KResult<u64> {
    let root_pa = new_root()?;
    crate::arch::smp::G_KERNEL_ROOT_PA = root_pa;
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

pub mod map;
pub mod walk;
pub mod unmap;

pub use map::{map, map_anon, map_one_pub};
pub use unmap::*;
