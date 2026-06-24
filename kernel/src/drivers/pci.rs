//! PCI ECAM — single-pass bus scan for QEMU riscv64 virt machine.
//!
//! ECAM at 0x30000000 (hardcoded — QEMU virt fixed address). Scans buses 0–15
//! for display controllers (PCI class 0x03) and returns the framebuffer
//! physical address from the first valid PCI memory BAR.
//!
//! Stateless — no global, no struct. Caller passes addresses directly.
use crate::arch::mmio::Mmio;
use onyx_core::errno::{Errno, KResult};

const ECAM_BASE: usize = 0x30000000;

const PCI_VENDOR_ID: u32 = 0x00;
const PCI_CLASS_REV: u32 = 0x08;
const PCI_BAR0: u32 = 0x10;



unsafe fn cfg_rd(bus: u8, dev: u8, fun: u8, off: u32) -> u32 {
    let addr = ECAM_BASE
        + ((bus as usize) << 20)
        + ((dev as usize) << 15)
        + ((fun as usize) << 12)
        + (off as usize);
    Mmio::<u32>::at(addr).read()
}

pub unsafe fn find_vga_fb() -> KResult<usize> {
    for bus in 0u8..16 {
        // Probe dev 0, func 0 for any device on this bus
        let probe = cfg_rd(bus, 0, 0, PCI_VENDOR_ID) & 0xFFFF;
        if probe == 0xFFFF || probe == 0 {
            continue;
        }
        crate::kdbg!("pci", "bus=%d has devices", crate::srv::klog::FmtArg::from(bus as u32));
        for dev in 0u8..32 {
            let ven = cfg_rd(bus, dev, 0, PCI_VENDOR_ID) & 0xFFFF;
            if ven == 0xFFFF || ven == 0 {
                continue;
            }
            let cls = cfg_rd(bus, dev, 0, PCI_CLASS_REV) >> 16;
            crate::kdbg!("pci", "bus=%d dev=%d ven=%x class=%x", crate::srv::klog::FmtArg::from(bus as u32), crate::srv::klog::FmtArg::from(dev as u32), crate::srv::klog::FmtArg::from(ven), crate::srv::klog::FmtArg::from(cls));
            if (cls >> 8) == 0x03 {
                for bar_idx in 0u32..6 {
                    let off = PCI_BAR0 + bar_idx * 4;
                    let val = cfg_rd(bus, dev, 0, off);
                    if val == 0 || val == 0xFFFFFFFF {
                        continue;
                    }
                    let is_mem = (val & 1) == 0;
                    if !is_mem {
                        continue;
                    }
                    if (val & 0x6) == 0x4 {
                        if bar_idx + 1 >= 6 { continue; }
                        let hi = cfg_rd(bus, dev, 0, off + 4);
                        let fb_pa = ((val & 0xFFFF_FFF0) as u64) | ((hi as u64) << 32);
                        crate::kinf!(
                            "pci",
                            "VGA at %d:%d BAR%d=%p (64-bit)",
                            crate::srv::klog::FmtArg::from(bus as u32),
                            crate::srv::klog::FmtArg::from(dev as u32),
                            crate::srv::klog::FmtArg::from(bar_idx),
                            crate::srv::klog::FmtArg::from(fb_pa)
                        );
                        return Ok(fb_pa as usize);
                    } else {
                        let fb_pa = val & 0xFFFF_FFF0;
                        crate::kinf!(
                            "pci",
                            "VGA at %d:%d BAR%d=%p (32-bit)",
                            crate::srv::klog::FmtArg::from(bus as u32),
                            crate::srv::klog::FmtArg::from(dev as u32),
                            crate::srv::klog::FmtArg::from(bar_idx),
                            crate::srv::klog::FmtArg::from(fb_pa as u64)
                        );
                        if fb_pa == 0 {
                            crate::kwrn!("pci", "VGA BAR0 is zero, skipping");
                        } else {
                            return Ok(fb_pa as usize);
                        }
                    }
                }
                // fallback: if no BAR found, still report the device
                crate::kwrn!("pci", "VGA at %d:%d has no valid BAR",
                    crate::srv::klog::FmtArg::from(bus as u32),
                    crate::srv::klog::FmtArg::from(dev as u32));
            }
        }
    }
    Err(Errno::NoEnt)
}
