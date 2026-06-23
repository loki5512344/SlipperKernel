//! Integration tests for onyx_core.

extern crate alloc;
use onyx_core::formats::*;

#[test]
fn test_onx_v1_roundtrip() {
    let seg = OnxSegment {
        vaddr: 0x10000,
        filesz: 100,
        memsz: 200,
        offset: 344,
        flags: VMM_R | VMM_X,
        align: 4096,
        reserved: 0,
        compressed_size: 0,
    };
    let hdr = OnxHeader {
        magic: ONX_MAGIC,
        version: ONX_VERSION_1,
        entry: 0x10000,
        nsegs: 1,
        flags: ONX_FLAGS_RING1,
        segs: alloc::vec![seg],
    };
    let bytes = hdr.to_bytes_v1();
    let parsed = OnxHeader::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.entry, 0x10000);
    assert_eq!(parsed.flags, ONX_FLAGS_RING1);
    assert_eq!(parsed.version, 1);
}

#[test]
fn test_onx_v2_roundtrip() {
    let seg = OnxSegment {
        vaddr: 0x10000,
        filesz: 100,
        memsz: 200,
        offset: 32,
        flags: VMM_R | VMM_X,
        align: 4096,
        reserved: 0,
        compressed_size: 0,
    };
    let hdr = OnxHeader {
        magic: ONX_MAGIC,
        version: ONX_VERSION_2,
        entry: 0x10000,
        nsegs: 1,
        flags: ONX_FLAGS_RING1,
        segs: alloc::vec![seg],
    };
    let bytes = hdr.to_bytes_v2();
    let parsed = OnxHeader::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.entry, 0x10000);
    assert_eq!(parsed.version, 2);
    assert_eq!(parsed.segs.len(), 1);
}

#[test]
fn test_onyfs_super_v2_roundtrip() {
    let sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 2,
        block_size: 4096,
        total_blocks: 100,
        inode_count: 32,
        inode_table_start: 3,
        data_bitmap_start: 2,
        data_blocks_start: 4,
        root_inode: 1,
        snapshot_area_start: 0,
        snapshot_count: 0,
        journal_start: 0,
        journal_size: 0,
        feature_flags: ONYFS_FEAT_TIMESTAMPS,
        creation_time: 1234567890,
        last_mount_time: 0,
        reserved: [0; 10],
    };
    let bytes = sb.to_bytes();
    let parsed = OnyfsSuper::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, sb);
}
