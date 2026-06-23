//! mkimage — OnyxFS disk image builder with manifest support.
//!
//! Usage: mkimage <manifest> <output.img>
//!
//! Manifest format (one entry per line):
//!   dir <path>                          — create directory
//!   file <local_path> <fs_path> [--ring=1]  — add file
//!   # comment
//!
//! Example:
//!   dir /bin
//!   dir /etc
//!   dir /service
//!   file build/init.onx /bin/init --ring=1
//!   file build/osh.onx /bin/osh
//!   file build/login.onx /bin/login --ring=1

use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

const ONYFS_MAGIC: u32 = 0x31594E4F;
const ONYFS_BLOCK_SIZE: usize = 4096;
const ONYFS_NAME_MAX: usize = 32;
const ONYFS_DIRECT_BLKS: usize = 10;
const ONYFS_ROOT_INO: u32 = 1;
const ONYFS_DT_REG: u32 = 0o100755;
const ONYFS_DT_DIR: u32 = 0o040755;
const INODE_SIZE: usize = 64;
const DIRENT_SIZE: usize = 36;
const INODES_PER_BLOCK: usize = ONYFS_BLOCK_SIZE / INODE_SIZE;
#[expect(dead_code)]
const DIRENTS_PER_BLOCK: usize = ONYFS_BLOCK_SIZE / DIRENT_SIZE;
#[expect(dead_code)]
const MAX_INODES: u32 = 256;
#[expect(dead_code)]
const MAX_BLOCKS: u32 = 4096;

struct Entry {
    #[expect(dead_code)]
    name: String,
    inode: u32,
    #[expect(dead_code)]
    is_dir: bool,
    data: Vec<u8>,
    #[expect(dead_code)]
    ring1: bool,
}

struct DirNode {
    ino: u32,
    parent_ino: u32,
    entries: Vec<(String, u32, bool)>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: mkimage <manifest> <output.img>");
        eprintln!("Manifest format:");
        eprintln!("  dir /path              — create directory");
        eprintln!("  file local /fs/path [--ring=1]  — add file");
        eprintln!("  # comment");
        process::exit(1);
    }
    let manifest_path = &args[1];
    let output_path = &args[2];

    let manifest = std::fs::read_to_string(manifest_path).unwrap_or_else(|e| {
        eprintln!("read manifest {}: {}", manifest_path, e);
        process::exit(1);
    });

    let mut dirs: Vec<DirNode> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();
    let mut next_ino: u32 = 2;

    dirs.push(DirNode {
        ino: 1,
        parent_ino: 1,
        entries: Vec::new(),
    });

    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        match parts[0] {
            "dir" => {
                let path = parts.get(1).unwrap_or(&"/");
                let parent = find_parent_dir(&dirs, path);
                let name = basename(path);
                let ino = next_ino;
                next_ino += 1;
                dirs[parent].entries.push((name.to_string(), ino, true));
                dirs.push(DirNode {
                    ino,
                    parent_ino: dirs[parent].ino,
                    entries: Vec::new(),
                });
                eprintln!("  dir {} (ino={})", path, ino);
            }
            "file" => {
                let local = parts.get(1).unwrap_or(&"");
                let fs_path = parts
                    .get(2)
                    .unwrap_or(&"")
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                let ring1 = line.contains("--ring=1");
                let data = std::fs::read(local).unwrap_or_else(|e| {
                    eprintln!("read {}: {}", local, e);
                    process::exit(1);
                });
                let parent = find_parent_dir(&dirs, fs_path);
                let name = basename(fs_path);
                let ino = next_ino;
                next_ino += 1;
                dirs[parent].entries.push((name.to_string(), ino, false));
                files.push(Entry {
                    name: name.to_string(),
                    inode: ino,
                    is_dir: false,
                    data,
                    ring1,
                });
                eprintln!(
                    "  file {} -> {} (ino={}, {} bytes{})",
                    local,
                    fs_path,
                    ino,
                    files.last().unwrap().data.len(),
                    if ring1 { " [ring1]" } else { "" }
                );
            }
            _ => {
                eprintln!("warning: unknown manifest entry: {}", line);
            }
        }
    }

    let total_inodes = next_ino;
    let inode_table_blocks = (total_inodes as usize).div_ceil(INODES_PER_BLOCK) as u32;

    let mut data_blocks_needed: u32 = 0;
    for _d in &dirs {
        data_blocks_needed += 1;
    }
    for f in &files {
        data_blocks_needed += f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
    }

    let metadata_blocks = 3 + inode_table_blocks;
    let total_blocks = metadata_blocks + data_blocks_needed;
    let img_size = (total_blocks as usize * ONYFS_BLOCK_SIZE + 511) & !511;
    let mut img = vec![0u8; img_size];

    let inode_table_start = 3u32;
    let _data_bitmap_start = 2u32;
    let data_blocks_start = metadata_blocks;

    write_superblock(
        &mut img,
        total_blocks,
        total_inodes,
        inode_table_start,
        data_blocks_start,
    );
    write_inode_bitmap(&mut img, total_inodes);
    write_data_bitmap(&mut img, data_blocks_needed);
    write_inode_table(
        &mut img,
        &dirs,
        &files,
        data_blocks_start,
        inode_table_start,
    );
    write_data_blocks(
        &mut img,
        &dirs,
        &files,
        data_blocks_start,
        inode_table_start,
    );

    File::create(output_path)
        .unwrap_or_else(|e| {
            eprintln!("create {}: {}", output_path, e);
            process::exit(1);
        })
        .write_all(&img)
        .unwrap();
    eprintln!(
        "mkimage: {} -> {} ({} blocks, {} bytes, {} inodes)",
        manifest_path,
        output_path,
        total_blocks,
        img.len(),
        total_inodes
    );
}

fn find_parent_dir(dirs: &[DirNode], path: &str) -> usize {
    let path = path.trim_start_matches('/');
    if !path.contains('/') {
        return 0;
    }
    let parent_path = &path[..path.rfind('/').unwrap()];
    let components: Vec<&str> = parent_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut cur_idx = 0;
    for comp in components {
        let mut found = false;
        for d in dirs.iter() {
            if d.parent_ino == dirs[cur_idx].ino {
                for (name, _ino, _is_dir) in &dirs[cur_idx].entries {
                    if name == comp {
                        for (j, dd) in dirs.iter().enumerate() {
                            if dd.ino == *_ino {
                                cur_idx = j;
                                found = true;
                                break;
                            }
                        }
                        if found {
                            break;
                        }
                    }
                }
            }
            if found {
                break;
            }
        }
    }
    cur_idx
}

fn basename(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

fn write_superblock(
    img: &mut [u8],
    total_blocks: u32,
    inode_count: u32,
    inode_table_start: u32,
    data_blocks_start: u32,
) {
    let sb = [
        ONYFS_MAGIC.to_le_bytes(),
        1u32.to_le_bytes(),
        (ONYFS_BLOCK_SIZE as u32).to_le_bytes(),
        total_blocks.to_le_bytes(),
        inode_count.to_le_bytes(),
        inode_table_start.to_le_bytes(),
        2u32.to_le_bytes(),
        data_blocks_start.to_le_bytes(),
        ONYFS_ROOT_INO.to_le_bytes(),
    ];
    let mut off = 0;
    for chunk in &sb {
        img[off..off + 4].copy_from_slice(chunk);
        off += 4;
    }
}

fn write_inode_bitmap(img: &mut [u8], count: u32) {
    let off = ONYFS_BLOCK_SIZE;
    for i in 0..count {
        let byte_off = off + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
}

fn write_data_bitmap(img: &mut [u8], count: u32) {
    let off = 2 * ONYFS_BLOCK_SIZE;
    for i in 0..count {
        let byte_off = off + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
}

fn write_inode(img: &mut [u8], inode_off: usize, mode: u32, size: u32, blocks: &[u32]) {
    img[inode_off..inode_off + 4].copy_from_slice(&mode.to_le_bytes());
    img[inode_off + 4..inode_off + 8].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = inode_off + 8 + i * 4;
        img[off..off + 4].copy_from_slice(&blk.to_le_bytes());
    }
}

fn write_dirent(img: &mut [u8], off: usize, name: &str, inode: u32) {
    let bytes = name.as_bytes();
    let n = bytes.len().min(ONYFS_NAME_MAX);
    img[off..off + n].copy_from_slice(&bytes[..n]);
    img[off + 32..off + 36].copy_from_slice(&inode.to_le_bytes());
}

fn write_inode_table(
    img: &mut [u8],
    dirs: &[DirNode],
    files: &[Entry],
    data_blocks_start: u32,
    inode_table_start: u32,
) {
    let base = inode_table_start as usize * ONYFS_BLOCK_SIZE;
    let mut data_blk = data_blocks_start;

    for d in dirs {
        let dir_data_blk = data_blk;
        data_blk += 1;
        let off = base + (d.ino as usize - 1) * INODE_SIZE;
        write_inode(img, off, ONYFS_DT_DIR, 0, &[dir_data_blk]);
    }
    for f in files {
        let nblks = f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
        let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
        for i in 0..nblks.min(ONYFS_DIRECT_BLKS as u32) {
            blocks[i as usize] = data_blk;
            data_blk += 1;
        }
        let off = base + (f.inode as usize - 1) * INODE_SIZE;
        write_inode(img, off, ONYFS_DT_REG, f.data.len() as u32, &blocks);
    }
}

fn write_data_blocks(
    img: &mut [u8],
    dirs: &[DirNode],
    files: &[Entry],
    data_blocks_start: u32,
    inode_table_start: u32,
) {
    let _base = inode_table_start as usize * ONYFS_BLOCK_SIZE;
    let mut data_blk = data_blocks_start;

    for d in dirs {
        let dir_off = data_blk as usize * ONYFS_BLOCK_SIZE;
        data_blk += 1;
        let mut entry_off = dir_off;
        write_dirent(img, entry_off, ".", d.ino);
        entry_off += DIRENT_SIZE;
        write_dirent(img, entry_off, "..", d.parent_ino);
        entry_off += DIRENT_SIZE;
        for (name, ino, _is_dir) in &d.entries {
            if entry_off + DIRENT_SIZE > dir_off + ONYFS_BLOCK_SIZE {
                break;
            }
            write_dirent(img, entry_off, name, *ino);
            entry_off += DIRENT_SIZE;
        }
    }
    for f in files {
        let nblks = f.data.len().div_ceil(ONYFS_BLOCK_SIZE);
        for i in 0..nblks {
            let blk_off = data_blk as usize * ONYFS_BLOCK_SIZE;
            data_blk += 1;
            let start = i * ONYFS_BLOCK_SIZE;
            let end = (start + ONYFS_BLOCK_SIZE).min(f.data.len());
            img[blk_off..blk_off + end - start].copy_from_slice(&f.data[start..end]);
        }
    }
}
