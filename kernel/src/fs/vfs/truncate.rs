use super::{fd_check, fd_get, FdToken};
use crate::fs::onyxfs;
use onyx_core::errno::KResult;

pub unsafe fn truncate(token: FdToken) -> KResult<()> {
    let idx = fd_check(token)?;
    let fd = fd_get(idx);
    onyxfs::truncate(fd.ino)
}
