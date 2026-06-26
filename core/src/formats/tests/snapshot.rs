use super::super::*;

#[test]
fn test_snapshot_meta_roundtrip() {
    let mut name = [0u8; 32];
    name[..11].copy_from_slice(b"backup_root");
    let snap = SnapshotMeta {
        id: 1,
        timestamp: 1234567890,
        root_inode_snapshot: 1,
        block_count: 500,
        name,
        parent_id: 0,
        flags: 0,
        reserved: [0; 4],
    };
    let bytes = snap.to_bytes();
    let parsed = SnapshotMeta::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.id, 1);
    assert_eq!(&parsed.name[..11], b"backup_root");
}
