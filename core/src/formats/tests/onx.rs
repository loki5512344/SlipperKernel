use super::super::*;

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
