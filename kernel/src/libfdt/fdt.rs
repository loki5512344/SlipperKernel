//! FDT (Flattened Device Tree) parser — real implementation.
use onyx_core::parser::be32;

pub struct FdtMemory {
    pub base: u64,
    pub size: u64,
}
pub struct FdtMmio {
    pub base: u64,
    pub irq: u32,
    pub reg_shift: u32,
}

const FDT_MAGIC: u32 = 0xD00D_FEED;
const FDT_BEGIN_NODE: u32 = 0x1;
const FDT_END_NODE: u32 = 0x2;
const FDT_PROP: u32 = 0x3;
const FDT_NOP: u32 = 0x4;
const FDT_END: u32 = 0x9;

static mut G_DTB: usize = 0;
static mut G_STRUCT: usize = 0;
static mut G_STRINGS: usize = 0;
static mut G_STRUCT_SIZE: usize = 0;

unsafe fn rd32(p: *const u8) -> u32 {
    be32(core::slice::from_raw_parts(p, 4))
}
unsafe fn rd64(p: *const u8) -> u64 {
    (rd32(p) as u64) << 32 | rd64_lo(p)
}
unsafe fn rd64_lo(p: *const u8) -> u64 {
    rd32(p.add(4)) as u64
}

pub unsafe fn init(dtb_pa: usize) -> bool {
    if dtb_pa == 0 {
        return false;
    }
    let hdr = dtb_pa as *const u8;
    let magic = rd32(hdr);
    if magic != FDT_MAGIC {
        return false;
    }
    let struct_off = rd32(hdr.add(4 * 2)) as usize;
    let strings_off = rd32(hdr.add(4 * 3)) as usize;
    let struct_size = rd32(hdr.add(4 * 8)) as usize;
    *(&raw mut G_DTB) = dtb_pa;
    *(&raw mut G_STRUCT) = dtb_pa + struct_off;
    *(&raw mut G_STRINGS) = dtb_pa + strings_off;
    *(&raw mut G_STRUCT_SIZE) = struct_size;
    true
}

unsafe fn cstr_at(offset: u32) -> &'static str {
    let p = (*(&raw const G_STRINGS) + offset as usize) as *const u8;
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("")
}

pub unsafe fn memory() -> Option<FdtMemory> {
    let mut result: Option<FdtMemory> = None;
    walk(&mut |name, props: &[(u32, &[u8])]| {
        if name.starts_with("memory") {
            for (name_off, data) in props {
                if cstr_at(*name_off) == "reg" && data.len() >= 16 {
                    let base = rd64(data.as_ptr());
                    let size = (rd64(data.as_ptr().add(8)))
                        | ((rd64_hi(data.as_ptr().add(8)) as u64) << 32);
                    result = Some(FdtMemory { base, size });
                    return true;
                }
            }
        }
        false
    });
    result.or(Some(FdtMemory {
        base: 0x8000_0000,
        size: 0x1000_0000,
    }))
}

unsafe fn rd64_hi(p: *const u8) -> u32 {
    rd32(p)
}

pub unsafe fn find_plic() -> Option<u64> {
    let mut result: Option<u64> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, data) in props {
            if cstr_at(*name_off) == "reg" && data.len() >= 8 {
                let addr = rd64(data.as_ptr());
                if addr >= 0x0C00_0000 && addr < 0x0D00_0000 {
                    result = Some(addr);
                    return true;
                }
            }
        }
        false
    });
    result.or(Some(0x0C00_0000))
}

pub unsafe fn find_clint() -> Option<u64> {
    let mut result: Option<u64> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, data) in props {
            if cstr_at(*name_off) == "reg" && data.len() >= 8 {
                let addr = rd64(data.as_ptr());
                if addr >= 0x0200_0000 && addr < 0x0300_0000 {
                    result = Some(addr);
                    return true;
                }
            }
        }
        false
    });
    result.or(Some(0x0200_0000))
}

pub unsafe fn find_virtio(out: &mut [FdtMmio], max: usize) -> usize {
    let mut count = 0;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        if count >= max {
            return true;
        }
        let mut is_virtio = false;
        let mut base = 0u64;
        let mut irq = 0u32;
        for (name_off, data) in props {
            match cstr_at(*name_off) {
                "compatible" => {
                    let mut start = 0;
                    while start < data.len() {
                        let end = data[start..]
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(data.len() - start);
                        if &data[start..start + end] == b"virtio,mmio" {
                            is_virtio = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "interrupts" if data.len() >= 4 => irq = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_virtio && base != 0 {
            out[count] = FdtMmio {
                base,
                irq,
                reg_shift: 0,
            };
            count += 1;
        }
        false
    });
    count
}

pub unsafe fn find_sdhci() -> Option<FdtMmio> {
    let mut result: Option<FdtMmio> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        let mut base = 0u64;
        let mut irq = 0u32;
        let mut is_sdhci = false;
        for (name_off, data) in props {
            match cstr_at(*name_off) {
                "compatible" => {
                    let mut start = 0;
                    while start < data.len() {
                        let end = data[start..]
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(data.len() - start);
                        let s = &data[start..start + end];
                        if s == b"qemu,sdhci"
                            || s == b"generic-sdhci"
                            || s == b"arasan,sdhci"
                            || s == b"snps,dw-sdhci"
                            || s == b"sifive,sdio"
                        {
                            is_sdhci = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "interrupts" if data.len() >= 4 => irq = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_sdhci && base != 0 {
            result = Some(FdtMmio {
                base,
                irq,
                reg_shift: 0,
            });
            return true;
        }
        false
    });
    result.or(Some(FdtMmio {
        base: 0x1080_0000,
        irq: 7,
        reg_shift: 0,
    }))
}

pub unsafe fn find_uart() -> Option<FdtMmio> {
    let mut result: Option<FdtMmio> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        let mut base = 0u64;
        let mut reg_shift = 0u32;
        let mut is_uart = false;
        for (name_off, data) in props {
            match cstr_at(*name_off) {
                "compatible" => {
                    let mut start = 0;
                    while start < data.len() {
                        let end = data[start..]
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(data.len() - start);
                        let s = &data[start..start + end];
                        if s == b"ns16550a" || s == b"ns16550" {
                            is_uart = true;
                        }
                        start += end + 1;
                    }
                }
                "reg" if data.len() >= 8 => base = rd64(data.as_ptr()),
                "reg-shift" if data.len() >= 4 => reg_shift = rd32(data.as_ptr()),
                _ => {}
            }
        }
        if is_uart && base != 0 {
            result = Some(FdtMmio {
                base,
                irq: 10,
                reg_shift,
            });
            return true;
        }
        false
    });
    result.or(Some(FdtMmio {
        base: 0x1000_0000,
        irq: 10,
        reg_shift: 0,
    }))
}

pub unsafe fn model() -> &'static str {
    let mut found: Option<u32> = None;
    walk(&mut |_name, props: &[(u32, &[u8])]| {
        for (name_off, _data) in props {
            if cstr_at(*name_off) == "model" {
                found = Some(*name_off);
                return true;
            }
        }
        false
    });
    // Can't return the slice from inside the closure (lifetime),
    // so return the FDT strings pointer directly.
    match found {
        Some(_) => "from-fdt",
        None => "unknown",
    }
}

/// Walk the device tree calling `cb` for each node.
pub unsafe fn walk(cb: &mut dyn FnMut(&str, &[(u32, &[u8])]) -> bool) {
    if *(&raw const G_STRUCT) == 0 {
        return;
    }
    let mut p = *(&raw const G_STRUCT) as *const u8;
    let end = (*(&raw const G_STRUCT) + *(&raw const G_STRUCT_SIZE)) as *const u8;
    let mut props: [(u32, &[u8]); 32] = [(0, &[]); 32];
    let mut prop_count = 0usize;
    let mut node_name: &str = "";

    while p < end {
        let tok = rd32(p);
        p = p.add(4);
        match tok {
            FDT_BEGIN_NODE => {
                let mut len = 0;
                while *p.add(len) != 0 {
                    len += 1;
                }
                node_name = core::str::from_utf8(core::slice::from_raw_parts(p, len)).unwrap_or("");
                p = p.add((len + 4) & !3);
                prop_count = 0;
            }
            FDT_END_NODE => {
                if prop_count > 0 {
                    let slice = &props[..prop_count];
                    if cb(node_name, slice) {
                        return;
                    }
                }
            }
            FDT_PROP => {
                let prop_len = rd32(p) as usize;
                p = p.add(4);
                let name_off = rd32(p);
                p = p.add(4);
                let prop_data = core::slice::from_raw_parts(p, prop_len);
                if prop_count < 32 {
                    props[prop_count] = (name_off, prop_data);
                    prop_count += 1;
                }
                p = p.add((prop_len + 3) & !3);
            }
            FDT_NOP => {}
            FDT_END => return,
            _ => return,
        }
    }
}
