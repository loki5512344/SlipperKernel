pub mod read;
pub mod write;

pub(super) use read::read_inode;
pub use read::stat;
pub(super) use write::write_inode;
pub use write::update_mtime;
pub use write::set_timestamps;
