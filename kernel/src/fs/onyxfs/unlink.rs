use super::alloc::remove_dirent;
use super::journal::journal_commit;
use super::lookup::lookup;
use super::OnyfsStat;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

pub unsafe fn unlink(path: &[u8]) -> KResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(Errno::Inval);
    }
    if path.len() == 1 {
        return Err(Errno::Perm);
    }
    let trimmed = &path[1..];
    let last_slash = trimmed.iter().rposition(|&b| b == b'/');
    let (parent_path, filename) = match last_slash {
        Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
        None => (&b""[..], trimmed),
    };
    if filename.is_empty() {
        return Err(Errno::Inval);
    }
    let parent_ino = if parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        let mut st = OnyfsStat::default();
        lookup(parent_path, &mut st)?;
        st.ino
    };
    remove_dirent(parent_ino, filename)?;
    journal_commit()?;
    Ok(())
}
