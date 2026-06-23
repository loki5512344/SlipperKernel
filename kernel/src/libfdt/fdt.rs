//! FDT parser (simplified — hardcoded QEMU values for MVP).
use onyx_core::parser::be32;

pub struct FdtMemory {
    pub base: u64,
    pub size: u64,
}

static mut G_DTB: usize = 0;

pub unsafe fn init(dtb_pa: usize) -> bool {
    if dtb_pa == 0 {
        return false;
    }
    let magic = be32(core::slice::from_raw_parts(dtb_pa as *const u8, 4));
    if magic != 0xD00D_FEED {
        return false;
    }
    G_DTB = dtb_pa;
    true
}

pub unsafe fn memory() -> Option<FdtMemory> {
    Some(FdtMemory {
        base: 0x8000_0000,
        size: 0x1000_0000,
    })
}

pub unsafe fn find_plic() -> Option<u64> {
    Some(0x0C00_0000)
}
pub unsafe fn find_clint() -> Option<u64> {
    Some(0x0200_0000)
}
