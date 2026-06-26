use super::super::*;

#[test]
fn test_onyfs_v2_super_roundtrip() {
    let sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 2,
        block_size: 4096,
        total_blocks: 1000,
        inode_count: 128,
        inode_table_start: 5,
        data_bitmap_start: 3,
        data_blocks_start: 6,
        root_inode: 1,
        snapshot_area_start: 900,
        snapshot_count: 0,
        journal_start: 950,
        journal_size: 10,
        feature_flags: ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS,
        creation_time: 1234567890,
        last_mount_time: 1234567891,
        reserved: [0; 10],
    };
    let bytes = sb.to_bytes();
    let parsed = OnyfsSuper::from_bytes(&bytes).unwrap();
    assert_eq!(
        parsed.feature_flags,
        ONYFS_FEAT_TIMESTAMPS | ONYFS_FEAT_SNAPSHOTS
    );
    assert_eq!(parsed.snapshot_area_start, 900);
}

#[test]
fn test_onyfs_v2_inode_roundtrip() {
    let inode = OnyfsInode {
        mode: ONYFS_DT_REG,
        size: 0x100000,
        uid: 0,
        gid: 0,
        nlink: 1,
        blocks: {
            let mut b = [0u32; ONYFS_DIRECT_BLKS];
            b[0] = 10;
            b[1] = 11;
            b
        },
        indirect: 20,
        double_indirect: 0,
        crtime: 1000,
        mtime: 2000,
        atime: 3000,
        ctime: 4000,
        flags: 0,
        reserved: 0,
    };
    let bytes = inode.to_bytes();
    let parsed = OnyfsInode::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.size, 0x100000);
    assert_eq!(parsed.crtime, 1000);
    assert_eq!(parsed.mtime, 2000);
    assert_eq!(parsed.blocks[0], 10);
}
