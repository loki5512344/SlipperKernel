//! On-disk format definitions — split from the old monolithic `formats.rs`.
//!
//! Modules:
//!   - `header`    — OnyxExec v1/v2 binary format (OnxHeader, OnxSegment)
//!   - `onyxfs_fmt` — OnyxFS v2 on-disk structs (super, inode, dirent)
//!   - `snapshot`  — Snapshot metadata
//!   - `misc`      — FAT32 BPB, 8.3 name helpers

pub mod header;
pub mod misc;
pub mod onyxfs_fmt;
pub mod snapshot;
pub mod tests;

// Re-export everything for backward compatibility.
pub use header::*;
pub use misc::*;
pub use onyxfs_fmt::*;
pub use snapshot::*;
