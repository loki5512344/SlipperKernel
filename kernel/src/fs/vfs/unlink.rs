use super::resolve_mount;
use crate::fs::onyxfs;
use onyx_core::errno::{Errno, KResult};

pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let name = &path[1..];
    let (fs, _) = resolve_mount(name);
    if fs != super::Fs::Onyx {
        return Err(Errno::NoSys);
    }
    onyxfs::unlink(path)
}
