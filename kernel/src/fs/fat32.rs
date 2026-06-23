//! FAT32 read-only driver (simplified).
use crate::drivers::{virtio, virtio_req};
use core::ptr;
use onyx_core::errno::{Errno, KResult};
static mut G_DEV: usize = 0;
static mut G_SPC: u32 = 0;
static mut G_RESVD: u32 = 0;
static mut G_FAT_SZ: u32 = 0;
static mut G_ROOT_CLUSTER: u32 = 0;
static mut G_DATA_LBA: u32 = 0;
static mut G_SEC: [u8; 512] = [0; 512];

unsafe fn read_sec(lba: u64, buf: &mut [u8; 512]) -> KResult<()> {
    virtio_req::read(G_DEV, lba, buf.as_mut_ptr())
}
unsafe fn cluster_to_lba(cluster: u32) -> u64 {
    (G_DATA_LBA as u64) + ((cluster - 2) as u64) * (G_SPC as u64)
}
unsafe fn fat_next(cluster: u32) -> u32 {
    let fat_offset = (cluster as u64) * 4;
    let fat_lba = (G_RESVD as u64) + (fat_offset / 512);
    let _ = read_sec(fat_lba, &mut G_SEC);
    let off = (fat_offset % 512) as usize;
    u32::from_le_bytes([G_SEC[off], G_SEC[off + 1], G_SEC[off + 2], G_SEC[off + 3]]) & 0x0FFF_FFFF
}

pub unsafe fn mount(dev: usize) -> KResult<()> {
    G_DEV = dev;
    let mut bpb = [0u8; 512];
    read_sec(0, &mut bpb)?;
    if bpb[510] != 0x55 || bpb[511] != 0xAA {
        return Err(Errno::Inval);
    }
    let bps = u16::from_le_bytes([bpb[11], bpb[12]]) as u32;
    if bps != 512 {
        return Err(Errno::Inval);
    }
    G_SPC = bpb[13] as u32;
    G_RESVD = u16::from_le_bytes([bpb[14], bpb[15]]) as u32;
    G_FAT_SZ = u16::from_le_bytes([bpb[22], bpb[23]]) as u32;
    if G_FAT_SZ == 0 {
        G_FAT_SZ = u32::from_le_bytes([bpb[36], bpb[37], bpb[38], bpb[39]]);
    }
    G_ROOT_CLUSTER = u32::from_le_bytes([bpb[44], bpb[45], bpb[46], bpb[47]]);
    G_DATA_LBA = G_RESVD + 2 * G_FAT_SZ;
    Ok(())
}

pub unsafe fn lookup(_path: &[u8], _out_cluster: &mut u32, _out_size: &mut u32) -> KResult<()> {
    Err(Errno::NoEnt)
}
pub unsafe fn read(_cluster: u32, _buf: *mut u8, _off: u32, _len: u32) -> KResult<u32> {
    Ok(0)
}
