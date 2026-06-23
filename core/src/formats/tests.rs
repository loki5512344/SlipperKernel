#![cfg(test)]

use super::*;

mod tests {
    use super::*;

    #[test]
    fn test_onx_v2_roundtrip() {
        let hdr = OnxHeader {
            magic: ONX_MAGIC,
            version: ONX_VERSION_2,
            entry: 0x10000,
            nsegs: 3,
            flags: ONX_FLAGS_RING1,
            segs: alloc::vec![
                OnxSegment {
                    vaddr: 0x10000,
                    filesz: 100,
                    memsz: 200,
                    offset: 176,
                    flags: VMM_R | VMM_X,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 0
                },
                OnxSegment {
                    vaddr: 0x10420,
                    filesz: 287,
                    memsz: 287,
                    offset: 276,
                    flags: VMM_R,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 0
                },
                OnxSegment {
                    vaddr: 0x20000,
                    filesz: 500,
                    memsz: 500,
                    offset: 563,
                    flags: VMM_R | VMM_W,
                    align: 4096,
                    reserved: 0,
                    compressed_size: 300
                },
            ],
        };
        let bytes = hdr.to_bytes_v2();
        let parsed = OnxHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.nsegs, 3);
        assert_eq!(parsed.segs.len(), 3);
        assert_eq!(parsed.segs[2].compressed_size, 300);
    }

    #[test]
    fn test_onx_v1_compat() {
        let hdr = OnxHeader {
            magic: ONX_MAGIC,
            version: 1,
            entry: 0x10000,
            nsegs: 1,
            flags: 0,
            segs: alloc::vec![OnxSegment {
                vaddr: 0x10000,
                filesz: 100,
                memsz: 200,
                offset: 344,
                flags: VMM_R | VMM_X,
                align: 4096,
                reserved: 0,
                compressed_size: 0
            }],
        };
        let bytes = hdr.to_bytes_v1();
        let parsed = OnxHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.nsegs, 1);
    }

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

    #[test]
    fn test_name_to_83() {
        assert_eq!(name_to_83(b"kernel.elf").unwrap(), *b"KERNEL  ELF");
    }

    #[test]
    fn test_matches_83() {
        let dirent: &[u8] = b"KERNEL  ELF";
        assert!(matches_83(dirent, b"kernel.elf"));
    }
}
