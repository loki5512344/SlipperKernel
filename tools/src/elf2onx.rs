//! elf2onx — ELF64 RISC-V → .onx converter with --ring, --v1 and --v2 flags.
//!
//! Output formats:
//!   v2 (default): 32-byte header + dynamic segs × 48 bytes (adds compressed_size).
//!   v1 (--v1):    344-byte header (24 fixed + 8 segments × 40 bytes), max 8 segs.
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::process;

const ONX_MAGIC: u32 = 0x31584E4F;
const ONX_VERSION_1: u32 = 1;
const ONX_VERSION_2: u32 = 2;
const ONX_MAX_SEGS_V1: usize = 8;
const ONX_MAX_SEGS_V2: usize = 256;
const ONX_FLAGS_RING1: u32 = 0x2;
const VMM_R: u32 = 1 << 1;
const VMM_W: u32 = 1 << 2;
const VMM_X: u32 = 1 << 3;

// v1 header layout: 24-byte fixed + 8 × 40-byte segments = 344 bytes.
const V1_FIXED_HDR: usize = 24;
const V1_SEG_SIZE: usize = 40;
const V1_HEADER_SIZE: usize = V1_FIXED_HDR + ONX_MAX_SEGS_V1 * V1_SEG_SIZE; // 344

// v2 header layout: 32-byte fixed + nsegs × 48-byte segments.
const V2_FIXED_HDR: usize = 32;
const V2_SEG_SIZE: usize = 48;

#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
struct OnxSegment {
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    offset: u32,
    flags: u32,
    align: u32,
    reserved: u32,
    compressed_size: u32, // v2 only
}

impl OnxSegment {
    fn to_bytes_v1(self) -> [u8; V1_SEG_SIZE] {
        let mut b = [0u8; V1_SEG_SIZE];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b
    }

    fn to_bytes_v2(self) -> [u8; V2_SEG_SIZE] {
        let mut b = [0u8; V2_SEG_SIZE];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b[40..44].copy_from_slice(&self.compressed_size.to_le_bytes());
        // 44..48 pad/reserved
        b
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: elf2onx [--ring=1] [--v1] <input.elf> <output.onx>");
        process::exit(1);
    }
    let mut ring1 = false;
    let mut v1 = false;
    let mut input = String::new();
    let mut output = String::new();
    for arg in &args[1..] {
        if arg == "--ring=1" {
            ring1 = true;
        } else if arg == "--v1" {
            v1 = true;
        } else if input.is_empty() {
            input = arg.clone();
        } else {
            output = arg.clone();
        }
    }
    if input.is_empty() || output.is_empty() {
        eprintln!("usage: elf2onx [--ring=1] [--v1] <input.elf> <output.onx>");
        process::exit(1);
    }
    let v2 = !v1;

    let mut elf_data = Vec::new();
    File::open(&input)
        .unwrap_or_else(|e| {
            eprintln!("open {}: {}", input, e);
            process::exit(1);
        })
        .read_to_end(&mut elf_data)
        .unwrap_or_else(|e| {
            eprintln!("read {}: {}", input, e);
            process::exit(1);
        });

    if elf_data.len() < 64 || &elf_data[0..4] != b"\x7fELF" {
        eprintln!("not an ELF file");
        process::exit(1);
    }
    if elf_data[4] != 2 {
        eprintln!("not ELF64");
        process::exit(1);
    }
    if elf_data[5] != 1 {
        eprintln!("not little-endian");
        process::exit(1);
    }
    let e_type = u16::from_le_bytes([elf_data[16], elf_data[17]]);
    if e_type != 2 {
        eprintln!("not ET_EXEC");
        process::exit(1);
    }
    let e_machine = u16::from_le_bytes([elf_data[18], elf_data[19]]);
    if e_machine != 243 {
        eprintln!("not RISC-V");
        process::exit(1);
    }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes([elf_data[54], elf_data[55]]) as usize;
    let e_phnum = u16::from_le_bytes([elf_data[56], elf_data[57]]) as usize;

    let max_segs = if v2 { ONX_MAX_SEGS_V2 } else { ONX_MAX_SEGS_V1 };
    let mut segs: Vec<OnxSegment> = Vec::with_capacity(max_segs);

    // Compute the offset where the first segment's data begins.
    // For v1: fixed 344-byte header. For v2: 32 + nsegs*48, but nsegs
    // isn't known yet — we compute it in two passes.
    // First pass: collect PT_LOAD segments and record ELF offsets/sizes.
    struct LoadInfo {
        p_offset: usize,
        p_filesz: usize,
        seg: OnxSegment,
    }
    let mut loads: Vec<LoadInfo> = Vec::with_capacity(max_segs);

    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > elf_data.len() {
            break;
        }
        let p_type = u32::from_le_bytes([
            elf_data[off],
            elf_data[off + 1],
            elf_data[off + 2],
            elf_data[off + 3],
        ]);
        if p_type != 1 {
            continue;
        } // PT_LOAD
        if loads.len() >= max_segs {
            break;
        }
        let p_flags = u32::from_le_bytes(elf_data[off + 4..off + 8].try_into().unwrap());
        let p_vaddr = u64::from_le_bytes(elf_data[off + 16..off + 24].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(elf_data[off + 32..off + 40].try_into().unwrap());
        let p_memsz = u64::from_le_bytes(elf_data[off + 40..off + 48].try_into().unwrap());
        let p_align = u64::from_le_bytes(elf_data[off + 48..off + 56].try_into().unwrap());
        let p_offset = u64::from_le_bytes(elf_data[off + 8..off + 16].try_into().unwrap()) as usize;
        let mut flags = 0u32;
        if p_flags & 4 != 0 {
            flags |= VMM_R;
        }
        if p_flags & 2 != 0 {
            flags |= VMM_W;
        }
        if p_flags & 1 != 0 {
            flags |= VMM_X;
        }
        let seg = OnxSegment {
            vaddr: p_vaddr,
            filesz: p_filesz,
            memsz: p_memsz,
            offset: 0, // patched below
            flags,
            align: p_align as u32,
            reserved: 0,
            compressed_size: 0,
        };
        loads.push(LoadInfo {
            p_offset,
            p_filesz: p_filesz as usize,
            seg,
        });
    }

    let nsegs = loads.len() as u32;
    let data_start: u32 = if v2 {
        (V2_FIXED_HDR + nsegs as usize * V2_SEG_SIZE) as u32
    } else {
        V1_HEADER_SIZE as u32
    };

    // Assign offsets and gather final segments.
    let mut filesz_acc: u32 = data_start;
    for li in &loads {
        let mut s = li.seg;
        s.offset = filesz_acc;
        filesz_acc = filesz_acc.saturating_add(li.p_filesz as u32);
        segs.push(s);
    }

    let mut out = File::create(&output).unwrap_or_else(|e| {
        eprintln!("create {}: {}", output, e);
        process::exit(1);
    });

    if v2 {
        // v2 header: magic(4) + version(4=2) + entry(8) + nsegs(4) + flags(4) + reserved(8) = 32
        let mut hdr = [0u8; V2_FIXED_HDR];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_2.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        let flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
        hdr[20..24].copy_from_slice(&flags.to_le_bytes());
        // 24..32 reserved (already zero)
        out.write_all(&hdr).unwrap();
        for s in &segs {
            out.write_all(&s.to_bytes_v2()).unwrap();
        }
    } else {
        // v1 header: 344 bytes total.
        let mut hdr = [0u8; V1_HEADER_SIZE];
        hdr[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
        hdr[4..8].copy_from_slice(&ONX_VERSION_1.to_le_bytes());
        hdr[8..16].copy_from_slice(&e_entry.to_le_bytes());
        hdr[16..20].copy_from_slice(&nsegs.to_le_bytes());
        let flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
        hdr[20..24].copy_from_slice(&flags.to_le_bytes());
        for (i, s) in segs.iter().enumerate() {
            let off = V1_FIXED_HDR + i * V1_SEG_SIZE;
            hdr[off..off + V1_SEG_SIZE].copy_from_slice(&s.to_bytes_v1());
        }
        out.write_all(&hdr).unwrap();
    }

    // Write segment data.
    for li in &loads {
        let p_offset = li.p_offset;
        let p_filesz = li.p_filesz;
        if p_offset + p_filesz <= elf_data.len() {
            out.write_all(&elf_data[p_offset..p_offset + p_filesz])
                .unwrap();
        }
    }

    eprintln!(
        "elf2onx: {} -> {} (v{}, entry=0x{:x}, nsegs={}, ring={})",
        input,
        output,
        if v2 { 2 } else { 1 },
        e_entry,
        nsegs,
        if ring1 { 1 } else { 2 }
    );
}
