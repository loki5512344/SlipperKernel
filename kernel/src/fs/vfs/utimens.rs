use super::{resolve_mount, Fs};
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn utimens(path: &[u8], mtime: u64, atime: u64) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let (fs, _) = resolve_mount(name);
    if fs != Fs::Onyx {
        return Err(Errno::NoSys);
    }
    let mut st = onyxfs::OnyfsStat::default();
    let ino = onyxfs::lookup(name, &mut st)?;
    onyxfs::set_timestamps(ino, mtime, atime)
}
