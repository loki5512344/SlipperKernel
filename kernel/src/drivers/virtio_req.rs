//! virtio-blk request submission + polled I/O.
use crate::drivers::virtio::*;
use core::ptr;
use core::sync::atomic::{fence, Ordering};
use onyx_core::errno::{Errno, KResult};

unsafe fn submit_and_wait(dev_idx: usize, req_type: u32, sector: u64) -> KResult<()> {
    let pd = &raw mut G_DEVS;
    let dev = &mut (*pd)[dev_idx];
    (*dev.req_buf).req_type = req_type;
    (*dev.req_buf).reserved = 0;
    (*dev.req_buf).sector = sector;
    (*dev.req_buf).status = 0xFF;
    let req_pa = dev.req_buf as u64;
    let data_off = 16;
    let status_off = data_off + VIRTIO_BLK_SECTOR;
    (*dev.desc.add(0)) = VqDesc {
        addr: req_pa,
        len: 16,
        flags: VQ_DESC_F_NEXT,
        next: 1,
    };
    let data_flags = if req_type == VIRTIO_BLK_T_IN {
        VQ_DESC_F_NEXT | VQ_DESC_F_WRITE
    } else {
        VQ_DESC_F_NEXT
    };
    (*dev.desc.add(1)) = VqDesc {
        addr: req_pa + data_off as u64,
        len: VIRTIO_BLK_SECTOR as u32,
        flags: data_flags,
        next: 2,
    };
    (*dev.desc.add(2)) = VqDesc {
        addr: req_pa + status_off as u64,
        len: 1,
        flags: VQ_DESC_F_WRITE,
        next: 0,
    };
    let idx = core::ptr::read_volatile(core::ptr::addr_of!((*dev.avail).idx));
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*dev.avail).ring[(idx as usize) % VIRTQ_SIZE]),
        0,
    );
    fence(Ordering::SeqCst);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*dev.avail).idx),
        idx.wrapping_add(1),
    );
    reg_w(dev.base, R_QUEUE_NOTIFY, 0);
    let used_idx_ptr = core::ptr::addr_of!((*dev.used).idx);
    #[allow(clippy::while_immutable_condition)]
    while core::ptr::read_volatile(used_idx_ptr) == dev.last_used {}
    dev.last_used = core::ptr::read_volatile(used_idx_ptr);
    if (*dev.req_buf).status == VIRTIO_BLK_S_OK {
        Ok(())
    } else {
        Err(Errno::Io)
    }
}

pub unsafe fn read(dev_idx: usize, lba: u64, buf: *mut u8) -> KResult<()> {
    submit_and_wait(dev_idx, VIRTIO_BLK_T_IN, lba)?;
    let pd = &raw const G_DEVS;
    ptr::copy_nonoverlapping(
        (*(*pd)[dev_idx].req_buf).data.as_ptr(),
        buf,
        VIRTIO_BLK_SECTOR,
    );
    Ok(())
}

pub unsafe fn write(dev_idx: usize, lba: u64, buf: *const u8) -> KResult<()> {
    let pd = &raw const G_DEVS;
    ptr::copy_nonoverlapping(
        buf,
        (*(*pd)[dev_idx].req_buf).data.as_mut_ptr(),
        VIRTIO_BLK_SECTOR,
    );
    submit_and_wait(dev_idx, VIRTIO_BLK_T_OUT, lba)
}

/// Read `n_sectors` consecutive 512-byte sectors starting at `lba` into `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of writable memory.
///
/// MVP implementation: loops over `read()` for each sector. The
/// infrastructure is here so a future scatter-gather optimization can replace
/// the loop with a single batched virtio-blk request.
pub unsafe fn read_multi(dev_idx: usize, lba: u64, n_sectors: u32, buf: *mut u8) -> KResult<()> {
    for i in 0u32..n_sectors {
        read(
            dev_idx,
            lba + i as u64,
            buf.add((i as usize) * VIRTIO_BLK_SECTOR),
        )?;
    }
    Ok(())
}

/// Write `n_sectors` consecutive 512-byte sectors starting at `lba` from `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of readable memory.
///
/// MVP implementation: loops over `write()` for each sector. Like
/// `read_multi`, this is the seam where a future scatter-gather optimization
/// would issue a single batched virtio-blk request.
pub unsafe fn write_multi(dev_idx: usize, lba: u64, n_sectors: u32, buf: *const u8) -> KResult<()> {
    for i in 0u32..n_sectors {
        write(
            dev_idx,
            lba + i as u64,
            buf.add((i as usize) * VIRTIO_BLK_SECTOR),
        )?;
    }
    Ok(())
}
