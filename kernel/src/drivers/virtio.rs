//! virtio-blk MMIO driver — legacy v1 + modern v2.
use crate::arch::mmio::Mmio;
use crate::mm::pmm;
use core::ptr;
use onyx_core::errno::{Errno, KResult};

pub const VIRTIO_MAX_DEVS: usize = 4;
pub const VIRTIO_BLK_SECTOR: usize = 512;
pub const VIRTQ_SIZE: usize = 256;
pub const R_MAGIC_VALUE: u32 = 0x00;
pub const R_VERSION: u32 = 0x04;
pub const R_DEVICE_ID: u32 = 0x08;
pub const R_HOST_FEATURES: u32 = 0x10;
pub const R_GUEST_FEATURES: u32 = 0x14;
pub const R_QUEUE_SEL: u32 = 0x30;
pub const R_QUEUE_NUM_MAX: u32 = 0x34;
pub const R_QUEUE_NUM: u32 = 0x38;
pub const R_QUEUE_ALIGN: u32 = 0x3C;
pub const R_QUEUE_PFN: u32 = 0x40;
pub const R_QUEUE_NOTIFY: u32 = 0x50;
pub const R_STATUS: u32 = 0x70;
pub const R_QUEUE_DESC_LOW: u32 = 0x80;
pub const R_QUEUE_DESC_HIGH: u32 = 0x84;
pub const R_QUEUE_AVAIL_LOW: u32 = 0x90;
pub const R_QUEUE_AVAIL_HIGH: u32 = 0x94;
pub const R_QUEUE_USED_LOW: u32 = 0xA0;
pub const R_QUEUE_USED_HIGH: u32 = 0xA4;
pub const R_QUEUE_ENABLE: u32 = 0xB0;
pub const VIRTIO_S_ACK: u32 = 1;
pub const VIRTIO_S_DRIVER: u32 = 2;
pub const VIRTIO_S_DRIVER_OK: u32 = 4;
pub const VIRTIO_S_FEATURES_OK: u32 = 8;
pub const VIRTIO_ID_BLK: u32 = 2;
pub const VIRTIO_BLK_T_IN: u32 = 0;
pub const VIRTIO_BLK_T_OUT: u32 = 1;
pub const VIRTIO_BLK_S_OK: u8 = 0;
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
pub const VQ_DESC_F_NEXT: u16 = 1;
pub const VQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}
#[repr(C)]
pub struct VqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; VIRTQ_SIZE],
    pub used_event: u16,
}
#[repr(C)]
pub struct VqUsedElem {
    pub idx: u32,
    pub len: u32,
}
#[repr(C)]
pub struct VqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VqUsedElem; VIRTQ_SIZE],
    pub avail_event: u16,
}
#[repr(C, packed)]
pub struct BlkReq {
    pub req_type: u32,
    pub reserved: u32,
    pub sector: u64,
    pub data: [u8; VIRTIO_BLK_SECTOR],
    pub status: u8,
}

#[derive(Clone, Copy)]
pub struct VirtioBlkDev {
    pub base: usize,
    pub modern: bool,
    pub version: u32,
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub last_used: u16,
    pub req_buf: *mut BlkReq,
}

pub(crate) static mut G_DEVS: [VirtioBlkDev; VIRTIO_MAX_DEVS] = [VirtioBlkDev {
    base: 0,
    modern: false,
    version: 0,
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    last_used: 0,
    req_buf: ptr::null_mut(),
}; VIRTIO_MAX_DEVS];
pub(crate) static mut G_NDEVS: usize = 0;

#[inline]
pub(crate) unsafe fn reg_w(base: usize, off: u32, v: u32) {
    Mmio::<u32>::at(base + off as usize).write(v);
}
#[inline]
pub(crate) unsafe fn reg_r(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

pub unsafe fn probe(base: usize) -> bool {
    let magic = reg_r(base, R_MAGIC_VALUE);
    if magic != 0x7472_6976 {
        return false;
    }
    reg_r(base, R_DEVICE_ID) == VIRTIO_ID_BLK
}

pub unsafe fn init(base: usize) -> KResult<usize> {
    let pn = &raw const G_NDEVS;
    if *pn >= VIRTIO_MAX_DEVS {
        return Err(Errno::NoMem);
    }
    let idx = *pn;
    let version = reg_r(base, R_VERSION);
    let modern = version >= 2;
    let dev = VirtioBlkDev {
        base,
        modern,
        version,
        desc: ptr::null_mut(),
        avail: ptr::null_mut(),
        used: ptr::null_mut(),
        last_used: 0,
        req_buf: ptr::null_mut(),
    };
    (*&raw mut G_DEVS)[idx] = dev;
    reg_w(base, R_STATUS, 0);
    reg_w(base, R_STATUS, VIRTIO_S_ACK | VIRTIO_S_DRIVER);
    let host_feat = reg_r(base, R_HOST_FEATURES);
    reg_w(base, R_GUEST_FEATURES, host_feat & 0x1FFF_FFFF);
    if modern {
        reg_w(
            base,
            R_STATUS,
            VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_FEATURES_OK,
        );
        if reg_r(base, R_STATUS) & VIRTIO_S_FEATURES_OK == 0 {
            return Err(Errno::Inval);
        }
    }
    setup_queue(idx)?;
    reg_w(
        base,
        R_STATUS,
        VIRTIO_S_ACK | VIRTIO_S_DRIVER | VIRTIO_S_DRIVER_OK,
    );
    (*&raw mut G_NDEVS) += 1;
    Ok(idx)
}

unsafe fn setup_queue(idx: usize) -> KResult<()> {
    let pd = &raw mut G_DEVS;
    let dev = &mut (*pd)[idx];
    reg_w(dev.base, R_QUEUE_SEL, 0);
    reg_w(dev.base, R_QUEUE_NUM, VIRTQ_SIZE as u32);
    if dev.modern {
        let desc_pa = pmm::alloc_zero()? as usize;
        let avail_pa = pmm::alloc_zero()? as usize;
        let used_pa = pmm::alloc_zero()? as usize;
        let req_pa = pmm::alloc_zero()? as usize;
        dev.desc = desc_pa as *mut VqDesc;
        dev.avail = avail_pa as *mut VqAvail;
        dev.used = used_pa as *mut VqUsed;
        dev.req_buf = req_pa as *mut BlkReq;
        dev.last_used = 0;
        reg_w(dev.base, R_QUEUE_DESC_LOW, desc_pa as u32);
        reg_w(dev.base, R_QUEUE_DESC_HIGH, ((desc_pa as u64) >> 32) as u32);
        reg_w(dev.base, R_QUEUE_AVAIL_LOW, avail_pa as u32);
        reg_w(
            dev.base,
            R_QUEUE_AVAIL_HIGH,
            ((avail_pa as u64) >> 32) as u32,
        );
        reg_w(dev.base, R_QUEUE_USED_LOW, used_pa as u32);
        reg_w(dev.base, R_QUEUE_USED_HIGH, ((used_pa as u64) >> 32) as u32);
        reg_w(dev.base, R_QUEUE_ENABLE, 1);
    } else {
        let contig_pa = pmm::alloc_n(3)? as usize;
        let desc_pa = contig_pa;
        let avail_pa = contig_pa + 4096;
        let used_pa = contig_pa + 2 * 4096;
        let req_pa = pmm::alloc_zero()? as usize;
        dev.desc = desc_pa as *mut VqDesc;
        dev.avail = avail_pa as *mut VqAvail;
        dev.used = used_pa as *mut VqUsed;
        dev.req_buf = req_pa as *mut BlkReq;
        dev.last_used = 0;
        reg_w(dev.base, R_QUEUE_ALIGN, 4096);
        reg_w(dev.base, R_QUEUE_PFN, (desc_pa / 4096) as u32);
    }
    Ok(())
}

pub fn count() -> usize {
    unsafe { *(&raw const G_NDEVS) }
}
pub unsafe fn dev(idx: usize) -> *mut VirtioBlkDev {
    let pn = &raw const G_NDEVS;
    if idx < *pn {
        let pd = &raw mut G_DEVS;
        &mut (*pd)[idx]
    } else {
        ptr::null_mut()
    }
}
