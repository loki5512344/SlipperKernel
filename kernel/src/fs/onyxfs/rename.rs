use super::alloc::{add_dirent, remove_dirent};
use super::journal::journal_commit;
use super::lookup::lookup;
use super::OnyfsStat;
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_ROOT_INO;

pub unsafe fn rename(old_path: &[u8], new_path: &[u8]) -> KResult<()> {
    if old_path.is_empty() || old_path[0] != b'/' || new_path.is_empty() || new_path[0] != b'/' {
        return Err(Errno::Inval);
    }
    let mut st = OnyfsStat::default();
    let old_ino = lookup(old_path, &mut st)?;
    let dtype = ((st.mode >> 12) & 0xF) as u8;

    let old_trimmed = &old_path[1..];
    let old_last_slash = old_trimmed.iter().rposition(|&b| b == b'/');
    let (old_parent_path, old_filename) = match old_last_slash {
        Some(pos) => (&old_trimmed[..pos], &old_trimmed[pos + 1..]),
        None => (&b""[..], old_trimmed),
    };

    let new_trimmed = &new_path[1..];
    let new_last_slash = new_trimmed.iter().rposition(|&b| b == b'/');
    let (new_parent_path, new_filename) = match new_last_slash {
        Some(pos) => (&new_trimmed[..pos], &new_trimmed[pos + 1..]),
        None => (&b""[..], new_trimmed),
    };

    if old_filename.is_empty() || new_filename.is_empty() {
        return Err(Errno::Inval);
    }

    let old_parent_ino = if old_parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        lookup(old_parent_path, &mut st)?;
        st.ino
    };

    let new_parent_ino = if new_parent_path.is_empty() {
        ONYFS_ROOT_INO
    } else {
        lookup(new_parent_path, &mut st)?;
        st.ino
    };

    add_dirent(new_parent_ino, new_filename, old_ino, dtype)?;
    remove_dirent(old_parent_ino, old_filename)?;
    journal_commit()?;
    Ok(())
}
