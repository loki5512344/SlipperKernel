//! mkimage — OnyxFS v2 disk image builder with manifest + --add / --add-dir.
//!
//! Produces v2 format images by default (128-byte superblock, 128-byte inodes
//! with timestamps, 40-byte dirents with dtype/name_len, snapshot area and
//! journal stubs). Use --v1 for the legacy 36/64/36 format.
//!
//! Usage:
//!   mkimage [--v1] <manifest> <output.img> [--add <host> <fs_path>]... [--add-dir <host_dir> <fs_prefix>]...
//!   mkimage [--v1] --add <host> <fs_path> [--add ...] <output.img>
//!
//! Manifest format (one entry per line):
//!   dir <path>                          — create directory
//!   file <local_path> <fs_path> [--ring=1]  — add file
//!   # comment

use std::env;
use std::fs::File;
use std::io::Write;
use std::process;

// ── OnyxFS v2 on-disk constants ────────────────────────────────────────
const ONYFS_MAGIC_V2: u32 = 0x32594E4F; // 'ONY2' LE — v2 magic
const ONYFS_MAGIC_V1: u32 = 0x31594E4F; // 'ONY1' LE — v1 compat
const ONYFS_BLOCK_SIZE: usize = 4096;
const ONYFS_NAME_MAX: usize = 32;
const ONYFS_DIRECT_BLKS: usize = 10;
const ONYFS_ROOT_INO: u32 = 1;
const ONYFS_DT_REG: u32 = 0o100755;
const ONYFS_DT_DIR: u32 = 0o040755;

// ── v1 on-disk sizes (legacy) ──────────────────────────────────────────
const V1_INODE_SIZE: usize = 64;
const V1_DIRENT_SIZE: usize = 36;
const V1_SUPERBLOCK_SIZE: usize = 36; // 9 × u32

// ── v2 on-disk sizes ───────────────────────────────────────────────────
const V2_INODE_SIZE: usize = 128;
const V2_DIRENT_SIZE: usize = 40;
const V2_SUPERBLOCK_SIZE: usize = 128;

// ── Snapshot and journal layout constants ──────────────────────────────
const SNAPSHOT_BLOCKS_EACH: u32 = 64; // blocks per snapshot slot
const MAX_SNAPSHOTS: u32 = 4; // maximum snapshot slots
const JOURNAL_BLOCKS: u32 = 32; // journal area size in blocks

struct Entry {
    inode: u32,
    data: Vec<u8>,
}

struct DirNode {
    ino: u32,
    parent_ino: u32,
    entries: Vec<(String, u32, bool)>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: mkimage [--v1] [<manifest>] <output.img> [--add <host> <fs_path>]... [--add-dir <host_dir> <fs_prefix>]...");
        eprintln!("Manifest format:");
        eprintln!("  dir /path              — create directory");
        eprintln!("  file local /fs/path [--ring=1]  — add file");
        eprintln!("  # comment");
        eprintln!("Example:");
        eprintln!("  mkimage manifest.txt disk.img --add-dir build/ /");
        process::exit(1);
    }

    // Parse --v1 flag
    let mut v1 = false;
    let mut arg_idx = 1;
    if arg_idx < args.len() && args[arg_idx] == "--v1" {
        v1 = true;
        arg_idx += 1;
    }

    let inode_size = if v1 { V1_INODE_SIZE } else { V2_INODE_SIZE };
    let dirent_size = if v1 { V1_DIRENT_SIZE } else { V2_DIRENT_SIZE };
    let superblock_size = if v1 { V1_SUPERBLOCK_SIZE } else { V2_SUPERBLOCK_SIZE };
    let inodes_per_block = ONYFS_BLOCK_SIZE / inode_size;

    let mut dirs: Vec<DirNode> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();
    let mut next_ino: u32 = 2;

    dirs.push(DirNode {
        ino: 1,
        parent_ino: 1,
        entries: Vec::new(),
    });

    let mut i = arg_idx;

    // If second arg exists and doesn't start with '--', treat arg[1] as manifest
    if i < args.len() && !args[i].starts_with("--") {
        let manifest_path = &args[i];
        i += 1;
        let manifest = std::fs::read_to_string(manifest_path).unwrap_or_else(|e| {
            eprintln!("read manifest {}: {}", manifest_path, e);
            process::exit(1);
        });
        for line in manifest.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            match parts[0] {
                "dir" => add_dir(&mut dirs, parts.get(1).unwrap_or(&"/"), &mut next_ino),
                "file" => {
                    let local = parts.get(1).unwrap_or(&"");
                    let fs_path = parts
                        .get(2)
                        .unwrap_or(&"")
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    add_file(&mut dirs, &mut files, local, fs_path, &mut next_ino);
                }
                _ => {
                    eprintln!("warning: unknown manifest entry: {}", line);
                }
            }
        }
    }

    let output_path;

    // Process --add and --add-dir flags; last positional arg is output
    loop {
        if i >= args.len() {
            eprintln!("missing output path");
            process::exit(1);
        }
        match args[i].as_str() {
            "--add" => {
                i += 1;
                if i + 1 >= args.len() {
                    eprintln!("--add requires <host> <fs_path>");
                    process::exit(1);
                }
                let host = &args[i];
                i += 1;
                let fs_path = &args[i];
                i += 1;
                add_file(&mut dirs, &mut files, host, fs_path, &mut next_ino);
            }
            "--add-dir" => {
                i += 1;
                if i + 1 >= args.len() {
                    eprintln!("--add-dir requires <host_dir> <fs_prefix>");
                    process::exit(1);
                }
                let host_dir = &args[i];
                i += 1;
                let fs_prefix = &args[i];
                i += 1;
                add_dir_recursive(&mut dirs, &mut files, host_dir, fs_prefix, &mut next_ino);
            }
            _ => {
                output_path = args[i].clone();
                i += 1;
                break;
            }
        }
    }

    if i < args.len() {
        eprintln!("warning: extra arguments ignored: {:?}", &args[i..]);
    }

    if files.is_empty() && dirs.len() == 1 {
        eprintln!("warning: no files or directories added to image");
    }

    let total_inodes = next_ino;
    let inode_table_blocks = (total_inodes as usize).div_ceil(inodes_per_block) as u32;

    let mut data_blocks_needed: u32 = 0;
    for _d in &dirs {
        data_blocks_needed += 1;
    }
    for f in &files {
        data_blocks_needed += f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
    }

    // v2 layout: superblock(1) + inode_bitmap(1) + data_bitmap(1) + inode_table +
    //            snapshot_area + journal + data_blocks
    // v1 layout: superblock(1) + inode_bitmap(1) + data_bitmap(1) + inode_table + data_blocks
    let snapshot_area_blocks: u32 = if !v1 { MAX_SNAPSHOTS * SNAPSHOT_BLOCKS_EACH } else { 0 };
    let journal_blocks: u32 = if !v1 { JOURNAL_BLOCKS } else { 0 };

    let metadata_blocks = 3 + inode_table_blocks + snapshot_area_blocks + journal_blocks;
    let total_blocks = metadata_blocks + data_blocks_needed;
    let img_size = (total_blocks as usize * ONYFS_BLOCK_SIZE + 511) & !511;
    let mut img = vec![0u8; img_size];

    let inode_table_start = 3u32;
    let _data_bitmap_start = 2u32;
    let snapshot_area_start = if !v1 { 3 + inode_table_blocks } else { 0 };
    let journal_start = if !v1 { snapshot_area_start + snapshot_area_blocks } else { 0 };
    let data_blocks_start = metadata_blocks;

    if v1 {
        write_superblock_v1(
            &mut img,
            total_blocks,
            total_inodes,
            inode_table_start,
            data_blocks_start,
        );
    } else {
        write_superblock_v2(
            &mut img,
            superblock_size,
            total_blocks,
            total_inodes,
            inode_table_start,
            data_blocks_start,
            snapshot_area_start,
            journal_start,
            journal_blocks,
        );
    }
    write_inode_bitmap(&mut img, total_inodes);
    write_data_bitmap(&mut img, data_blocks_needed, metadata_blocks);
    write_inode_table(
        &mut img,
        &dirs,
        &files,
        data_blocks_start,
        inode_table_start,
        inode_size,
        v1,
    );
    write_data_blocks(
        &mut img,
        &dirs,
        &files,
        data_blocks_start,
        dirent_size,
        v1,
    );

    File::create(&output_path)
        .unwrap_or_else(|e| {
            eprintln!("create {}: {}", output_path, e);
            process::exit(1);
        })
        .write_all(&img)
        .unwrap();
    eprintln!(
        "mkimage: v{} {} -> {} ({} blocks, {} bytes, {} inodes)",
        if v1 { 1 } else { 2 },
        &args[1],
        output_path,
        total_blocks,
        img.len(),
        total_inodes
    );
}

fn add_dir(dirs: &mut Vec<DirNode>, path: &str, next_ino: &mut u32) {
    let parent = find_parent_dir(dirs, path);
    let name = basename(path);
    let ino = *next_ino;
    *next_ino += 1;
    dirs[parent].entries.push((name.to_string(), ino, true));
    dirs.push(DirNode {
        ino,
        parent_ino: dirs[parent].ino,
        entries: Vec::new(),
    });
    eprintln!("  dir {} (ino={})", path, ino);
}

fn add_file(
    dirs: &mut Vec<DirNode>,
    files: &mut Vec<Entry>,
    host: &str,
    fs_path: &str,
    next_ino: &mut u32,
) {
    let data = std::fs::read(host).unwrap_or_else(|e| {
        eprintln!("read {}: {}", host, e);
        process::exit(1);
    });
    ensure_parent_dirs(dirs, fs_path, next_ino);
    let parent = find_parent_dir(dirs, fs_path);
    let name = basename(fs_path);
    let ino = *next_ino;
    *next_ino += 1;
    dirs[parent].entries.push((name.to_string(), ino, false));
    files.push(Entry {
        inode: ino,
        data,
    });
    eprintln!(
        "  {} -> {} (ino={}, {} bytes)",
        host,
        fs_path,
        ino,
        files.last().unwrap().data.len()
    );
}

fn add_dir_recursive(
    dirs: &mut Vec<DirNode>,
    files: &mut Vec<Entry>,
    host_dir: &str,
    fs_prefix: &str,
    next_ino: &mut u32,
) {
    let prefix = fs_prefix.trim_end_matches('/').to_string();
    let host_root = std::path::Path::new(host_dir);
    let mut stack = vec![host_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
            eprintln!("read_dir {}: {}", dir.display(), e);
            process::exit(1);
        }) {
            let entry = entry.unwrap_or_else(|e| {
                eprintln!("read_dir entry: {}", e);
                process::exit(1);
            });
            let entry_path = entry.path();
            if let Ok(relative) = entry_path.strip_prefix(host_root) {
                let fs_path = format!("{}/{}", prefix, relative.display());
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    add_dir(dirs, &fs_path, next_ino);
                    stack.push(entry_path);
                } else if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    add_file(
                        dirs,
                        files,
                        entry_path.to_str().unwrap(),
                        &fs_path,
                        next_ino,
                    );
                }
            }
        }
    }
}

/// Ensure all ancestor directories of `path` exist in `dirs`.
fn ensure_parent_dirs(dirs: &mut Vec<DirNode>, path: &str, next_ino: &mut u32) {
    let path = path.trim_start_matches('/');
    let mut cur = String::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == basename(path) {
            continue;
        }
        cur.push('/');
        cur.push_str(comp);
        // check if dir already exists
        let exists = dirs.iter().any(|d| {
            let name = basename(&cur);
            d.parent_ino == dirs[0].ino && d.entries.iter().any(|(n, _, _)| n == name)
        });
        if !exists {
            add_dir(dirs, &cur, next_ino);
        }
    }
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

// ── v1 superblock writer (legacy, 36 bytes) ────────────────────────────
fn write_superblock_v1(
    img: &mut [u8],
    total_blocks: u32,
    inode_count: u32,
    inode_table_start: u32,
    data_blocks_start: u32,
) {
    let sb = [
        ONYFS_MAGIC_V1.to_le_bytes(),
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

// ── v2 superblock writer (128 bytes, with snapshot/journal fields) ─────
fn write_superblock_v2(
    img: &mut [u8],
    _sb_size: usize,
    total_blocks: u32,
    inode_count: u32,
    inode_table_start: u32,
    data_blocks_start: u32,
    snapshot_area_start: u32,
    journal_start: u32,
    journal_size: u32,
) {
    // v2 superblock layout (128 bytes):
    //   0..4    magic (ONY2)
    //   4..8    version (2)
    //   8..12   block_size (4096)
    //   12..16  total_blocks
    //   16..20  inode_count
    //   20..24  inode_table_start
    //   24..28  data_bitmap_start
    //   28..32  data_blocks_start
    //   32..36  root_inode (1)
    //   36..40  snapshot_area_start
    //   40..44  snapshot_count (0)
    //   44..48  journal_start
    //   48..52  journal_size
    //   52..56  feature_flags
    //   56..64  creation_time (u64)
    //   64..72  last_mount_time (u64)
    //   72..128 reserved (zeros)

    let feature_flags: u32 = 0x1 | 0x2 | 0x8; // TIMESTAMPS | SNAPSHOTS | JOURNAL

    let mut sb = [0u8; V2_SUPERBLOCK_SIZE];
    sb[0..4].copy_from_slice(&ONYFS_MAGIC_V2.to_le_bytes());
    sb[4..8].copy_from_slice(&2u32.to_le_bytes());
    sb[8..12].copy_from_slice(&(ONYFS_BLOCK_SIZE as u32).to_le_bytes());
    sb[12..16].copy_from_slice(&total_blocks.to_le_bytes());
    sb[16..20].copy_from_slice(&inode_count.to_le_bytes());
    sb[20..24].copy_from_slice(&inode_table_start.to_le_bytes());
    sb[24..28].copy_from_slice(&2u32.to_le_bytes()); // data_bitmap_start
    sb[28..32].copy_from_slice(&data_blocks_start.to_le_bytes());
    sb[32..36].copy_from_slice(&ONYFS_ROOT_INO.to_le_bytes());
    sb[36..40].copy_from_slice(&snapshot_area_start.to_le_bytes());
    sb[40..44].copy_from_slice(&0u32.to_le_bytes()); // snapshot_count
    sb[44..48].copy_from_slice(&journal_start.to_le_bytes());
    sb[48..52].copy_from_slice(&journal_size.to_le_bytes());
    sb[52..56].copy_from_slice(&feature_flags.to_le_bytes());

    // creation_time: use a simple timestamp (seconds since epoch as nanoseconds)
    let creation_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    sb[56..64].copy_from_slice(&creation_time.to_le_bytes());
    // last_mount_time: 0 (never mounted yet)

    img[0..V2_SUPERBLOCK_SIZE].copy_from_slice(&sb);
}

fn write_inode_bitmap(img: &mut [u8], count: u32) {
    let off = ONYFS_BLOCK_SIZE;
    for i in 0..count {
        let byte_off = off + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
}

fn write_data_bitmap(img: &mut [u8], count: u32, _metadata_blocks: u32) {
    let off = 2 * ONYFS_BLOCK_SIZE;
    for i in 0..count {
        let byte_off = off + (i / 8) as usize;
        img[byte_off] |= 1 << (i % 8);
    }
}

// ── v1 inode writer (64 bytes) ─────────────────────────────────────────
fn write_inode_v1(img: &mut [u8], inode_off: usize, mode: u32, size: u32, blocks: &[u32]) {
    img[inode_off..inode_off + 4].copy_from_slice(&mode.to_le_bytes());
    img[inode_off + 4..inode_off + 8].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = inode_off + 8 + i * 4;
        img[off..off + 4].copy_from_slice(&blk.to_le_bytes());
    }
}

// ── v2 inode writer (128 bytes, with timestamps) ───────────────────────
fn write_inode_v2(img: &mut [u8], inode_off: usize, mode: u32, size: u64, blocks: &[u32], is_dir: bool) {
    // v2 inode layout (128 bytes):
    //   0..4    mode
    //   4..8    padding
    //   8..16   size (u64)
    //   16..56  blocks[10] (10 × 4 = 40 bytes)
    //   56..60  uid
    //   60..64  gid
    //   64..68  nlink
    //   68..72  flags
    //   72..76  reserved
    //   76..96  padding
    //   96..100 indirect
    //   100..104 double_indirect
    //   104..112 crtime (u64)
    //   112..120 mtime (u64)
    //   120..128 atime (u64)
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let mut buf = [0u8; V2_INODE_SIZE];
    buf[0..4].copy_from_slice(&mode.to_le_bytes());
    // 4..8 padding (zero)
    buf[8..16].copy_from_slice(&size.to_le_bytes());
    for (i, &blk) in blocks.iter().enumerate().take(ONYFS_DIRECT_BLKS) {
        let off = 16 + i * 4;
        buf[off..off + 4].copy_from_slice(&blk.to_le_bytes());
    }
    buf[56..60].copy_from_slice(&0u32.to_le_bytes()); // uid = 0 (root)
    buf[60..64].copy_from_slice(&0u32.to_le_bytes()); // gid = 0 (root)
    let nlink: u32 = if is_dir { 2 } else { 1 };
    buf[64..68].copy_from_slice(&nlink.to_le_bytes());
    buf[68..72].copy_from_slice(&0u32.to_le_bytes()); // flags
    // 96..100 indirect = 0
    // 100..104 double_indirect = 0
    buf[104..112].copy_from_slice(&now_ns.to_le_bytes()); // crtime
    buf[112..120].copy_from_slice(&now_ns.to_le_bytes()); // mtime
    buf[120..128].copy_from_slice(&now_ns.to_le_bytes()); // atime

    img[inode_off..inode_off + V2_INODE_SIZE].copy_from_slice(&buf);
}

// ── v1 dirent writer (36 bytes) ────────────────────────────────────────
fn write_dirent_v1(img: &mut [u8], off: usize, name: &str, inode: u32) {
    let bytes = name.as_bytes();
    let n = bytes.len().min(ONYFS_NAME_MAX);
    img[off..off + n].copy_from_slice(&bytes[..n]);
    img[off + 32..off + 36].copy_from_slice(&inode.to_le_bytes());
}

// ── v2 dirent writer (40 bytes, with dtype and name_len) ───────────────
fn write_dirent_v2(img: &mut [u8], off: usize, name: &str, inode: u32, is_dir: bool) {
    let bytes = name.as_bytes();
    let n = bytes.len().min(ONYFS_NAME_MAX);
    img[off..off + n].copy_from_slice(&bytes[..n]);
    img[off + 32..off + 36].copy_from_slice(&inode.to_le_bytes());
    // dtype: 1 = directory, 2 = regular file (simple encoding)
    img[off + 36] = if is_dir { 1 } else { 2 };
    img[off + 37] = n as u8;
    // 38..40 reserved (already zero)
}

fn write_inode_table(
    img: &mut [u8],
    dirs: &[DirNode],
    files: &[Entry],
    data_blocks_start: u32,
    inode_table_start: u32,
    inode_size: usize,
    v1: bool,
) {
    let base = inode_table_start as usize * ONYFS_BLOCK_SIZE;
    let mut data_blk = data_blocks_start;

    for d in dirs {
        let dir_data_blk = data_blk;
        data_blk += 1;
        let off = base + (d.ino as usize - 1) * inode_size;
        if v1 {
            write_inode_v1(img, off, ONYFS_DT_DIR, 0, &[dir_data_blk]);
        } else {
            write_inode_v2(img, off, ONYFS_DT_DIR, 0, &[dir_data_blk], true);
        }
    }
    for f in files {
        let nblks = f.data.len().div_ceil(ONYFS_BLOCK_SIZE) as u32;
        let mut blocks = [0u32; ONYFS_DIRECT_BLKS];
        for i in 0..nblks.min(ONYFS_DIRECT_BLKS as u32) {
            blocks[i as usize] = data_blk;
            data_blk += 1;
        }
        let off = base + (f.inode as usize - 1) * inode_size;
        if v1 {
            write_inode_v1(img, off, ONYFS_DT_REG, f.data.len() as u32, &blocks);
        } else {
            write_inode_v2(img, off, ONYFS_DT_REG, f.data.len() as u64, &blocks, false);
        }
    }
}

fn write_data_blocks(
    img: &mut [u8],
    dirs: &[DirNode],
    files: &[Entry],
    data_blocks_start: u32,
    dirent_size: usize,
    v1: bool,
) {
    let mut data_blk = data_blocks_start;

    for d in dirs {
        let dir_off = data_blk as usize * ONYFS_BLOCK_SIZE;
        data_blk += 1;
        let mut entry_off = dir_off;
        if v1 {
            write_dirent_v1(img, entry_off, ".", d.ino);
            entry_off += dirent_size;
            write_dirent_v1(img, entry_off, "..", d.parent_ino);
            entry_off += dirent_size;
        } else {
            write_dirent_v2(img, entry_off, ".", d.ino, true);
            entry_off += dirent_size;
            write_dirent_v2(img, entry_off, "..", d.parent_ino, true);
            entry_off += dirent_size;
        }
        for (name, ino, is_dir) in &d.entries {
            if entry_off + dirent_size > dir_off + ONYFS_BLOCK_SIZE {
                break;
            }
            if v1 {
                write_dirent_v1(img, entry_off, name, *ino);
            } else {
                write_dirent_v2(img, entry_off, name, *ino, *is_dir);
            }
            entry_off += dirent_size;
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
