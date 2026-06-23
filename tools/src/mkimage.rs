//! mkimage — OnyxFS disk image builder with directory support.
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::process;

const ONYFS_MAGIC: u32 = 0x31594E4F;
const ONYFS_BLOCK_SIZE: usize = 4096;
const ONYFS_NAME_MAX: usize = 32;
const ONYFS_DIRECT_BLKS: usize = 10;
const ONYFS_ROOT_INO: u32 = 1;
const ONYFS_DT_REG: u32 = 0o100755;
const ONYFS_DT_DIR: u32 = 0o040755;

#[repr(C)]
struct OnyfsSuper {
    magic: u32,
    version: u32,
    block_size: u32,
    total_blocks: u32,
    inode_count: u32,
    inode_table_start: u32,
    data_bitmap_start: u32,
    data_blocks_start: u32,
    root_inode: u32,
    reserved: [u32; 7],
}
impl OnyfsSuper {
    fn to_bytes(&self) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&self.version.to_le_bytes());
        b[8..12].copy_from_slice(&self.block_size.to_le_bytes());
        b[12..16].copy_from_slice(&self.total_blocks.to_le_bytes());
        b[16..20].copy_from_slice(&self.inode_count.to_le_bytes());
        b[20..24].copy_from_slice(&self.inode_table_start.to_le_bytes());
        b[24..28].copy_from_slice(&self.data_bitmap_start.to_le_bytes());
        b[28..32].copy_from_slice(&self.data_blocks_start.to_le_bytes());
        b[32..36].copy_from_slice(&self.root_inode.to_le_bytes());
        b
    }
}

fn inode_bytes(mode: u32, size: u32, blocks: &[u32]) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0..4].copy_from_slice(&mode.to_le_bytes());
    b[4..8].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = 8 + i * 4;
        b[off..off + 4].copy_from_slice(&blk.to_le_bytes());
    }
    b
}

fn dirent(name: &str, inode: u32) -> [u8; 36] {
    let mut b = [0u8; 36];
    let bytes = name.as_bytes();
    let copy_len = bytes.len().min(ONYFS_NAME_MAX);
    b[..copy_len].copy_from_slice(&bytes[..copy_len]);
    b[32..36].copy_from_slice(&inode.to_le_bytes());
    b
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: mkimage <init.onx> <disk.img>");
        process::exit(1);
    }
    let input = &args[1];
    let output = &args[2];

    let mut init_data = Vec::new();
    File::open(input)
        .unwrap_or_else(|e| {
            eprintln!("open {}: {}", input, e);
            process::exit(1);
        })
        .read_to_end(&mut init_data)
        .unwrap_or_else(|e| {
            eprintln!("read {}: {}", input, e);
            process::exit(1);
        });

    let init_blocks = init_data.len().div_ceil(ONYFS_BLOCK_SIZE);
    // Layout:
    // Block 0: superblock
    // Block 1: inode bitmap
    // Block 2: data bitmap
    // Block 3: inode table
    // Block 4: root dir data (dirents: ".", "bin")
    // Block 5..5+init_blocks: init file data
    // Block 5+init_blocks: /bin dir data (dirents: ".", "init")
    let root_dir_blk = 4u32;
    let init_data_start = 5u32;
    let bin_dir_blk = init_data_start + init_blocks as u32;
    let total_data_blocks = 1 + init_blocks as u32 + 1; // root dir + init data + bin dir
    let total_blocks = 4 + total_data_blocks;
    let img_size = ((total_blocks as usize) * ONYFS_BLOCK_SIZE + 511) & !511;
    let mut img = vec![0u8; img_size];

    // Block 0: superblock
    let sb = OnyfsSuper {
        magic: ONYFS_MAGIC,
        version: 1,
        block_size: ONYFS_BLOCK_SIZE as u32,
        total_blocks,
        inode_count: 32,
        inode_table_start: 3,
        data_bitmap_start: 2,
        data_blocks_start: 4,
        root_inode: ONYFS_ROOT_INO,
        reserved: [0; 7],
    };
    img[0..64].copy_from_slice(&sb.to_bytes());

    // Block 1: inode bitmap — inodes 1 (root), 2 (init), 3 (bin) used = 0b111
    img[ONYFS_BLOCK_SIZE] = 0b0000_0111;

    // Block 2: data bitmap
    let data_bm = (!0u8).wrapping_shr(8u32.saturating_sub(total_data_blocks));
    img[2 * ONYFS_BLOCK_SIZE] = data_bm;

    // Block 3: inode table
    let inode_off = 3 * ONYFS_BLOCK_SIZE;
    // Inode 1: root dir — mode=DIR, blocks[0]=4
    img[inode_off..inode_off + 64].copy_from_slice(&inode_bytes(ONYFS_DT_DIR, 72, &[root_dir_blk]));
    // Inode 2: /bin/init file — mode=REG, size=init_data.len()
    let mut init_blocks_arr = [0u32; ONYFS_DIRECT_BLKS];
    for (i, slot) in init_blocks_arr.iter_mut().enumerate().take(init_blocks) {
        *slot = init_data_start + i as u32;
    }
    let init_inode_off = inode_off + 64;
    img[init_inode_off..init_inode_off + 64].copy_from_slice(&inode_bytes(
        ONYFS_DT_REG,
        init_data.len() as u32,
        &init_blocks_arr,
    ));
    // Inode 3: /bin directory — mode=DIR, blocks[0]=bin_dir_blk
    let bin_inode_off = inode_off + 128;
    img[bin_inode_off..bin_inode_off + 64].copy_from_slice(&inode_bytes(
        ONYFS_DT_DIR,
        72,
        &[bin_dir_blk],
    ));

    // Block 4: root directory data — ".", "bin"
    let dir_off = root_dir_blk as usize * ONYFS_BLOCK_SIZE;
    img[dir_off..dir_off + 36].copy_from_slice(&dirent(".", 1));
    img[dir_off + 36..dir_off + 72].copy_from_slice(&dirent("bin", 3));

    // Block 5..: init file data
    let data_off = init_data_start as usize * ONYFS_BLOCK_SIZE;
    img[data_off..data_off + init_data.len()].copy_from_slice(&init_data);

    // Block bin_dir_blk: /bin directory data — ".", "init"
    let bin_dir_off = bin_dir_blk as usize * ONYFS_BLOCK_SIZE;
    img[bin_dir_off..bin_dir_off + 36].copy_from_slice(&dirent(".", 3));
    img[bin_dir_off + 36..bin_dir_off + 72].copy_from_slice(&dirent("init", 2));

    File::create(output)
        .unwrap_or_else(|e| {
            eprintln!("create {}: {}", output, e);
            process::exit(1);
        })
        .write_all(&img)
        .unwrap();
    eprintln!(
        "mkimage: {} -> {} ({} blocks, {} bytes)",
        input,
        output,
        total_blocks,
        img.len()
    );
}
