use crate::arch::csr;
use crate::arch::regs::*;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::KResult;

use super::walk::walk;

pub unsafe fn unmap(root_pa: u64, vaddr: u64, size: usize) -> KResult<()> {
    let mut va = vaddr;
    let mut remaining = size;
    while remaining > 0 {
        let pte_ptr = walk(root_pa, va, 0, false)?;
        let pte = ptr::read_volatile(pte_ptr);
        if pte & PTE_V != 0 && pte & PTE_U != 0 {
            let paddr = (pte & PTE_PPN_MASK) >> PTE_PPN_SHIFT << 12;
            pmm::free(paddr);
        }
        ptr::write_volatile(pte_ptr, 0);
        va += 4096;
        remaining -= 4096;
    }
    csr::sfence_vma_all();
    Ok(())
}
