//! elf2onx — ELF64 RISC-V → .onx converter with --ring flag.
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::process;

const ONX_MAGIC: u32 = 0x31584E4F;
const ONX_VERSION: u32 = 1;
const ONX_MAX_SEGS: usize = 8;
const ONX_FLAGS_RING1: u32 = 0x2;
const VMM_R: u32 = 1 << 1;
const VMM_W: u32 = 1 << 2;
const VMM_X: u32 = 1 << 3;

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
}
impl OnxSegment {
    fn to_bytes(self) -> [u8; 40] {
        let mut b = [0u8; 40];
        b[0..8].copy_from_slice(&self.vaddr.to_le_bytes());
        b[8..16].copy_from_slice(&self.filesz.to_le_bytes());
        b[16..24].copy_from_slice(&self.memsz.to_le_bytes());
        b[24..28].copy_from_slice(&self.offset.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b[32..36].copy_from_slice(&self.align.to_le_bytes());
        b[36..40].copy_from_slice(&self.reserved.to_le_bytes());
        b
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: elf2onx [--ring=1] <input.elf> <output.onx>");
        process::exit(1);
    }
    let mut ring1 = false;
    let mut input = String::new();
    let mut output = String::new();
    for arg in &args[1..] {
        if arg == "--ring=1" {
            ring1 = true;
        } else if input.is_empty() {
            input = arg.clone();
        } else {
            output = arg.clone();
        }
    }
    if input.is_empty() || output.is_empty() {
        eprintln!("usage: elf2onx [--ring=1] <input.elf> <output.onx>");
        process::exit(1);
    }

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

    let mut segs = [OnxSegment::default(); ONX_MAX_SEGS];
    let mut nsegs = 0u32;
    let mut filesz_acc: u32 = 344;
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
        if nsegs as usize >= ONX_MAX_SEGS {
            break;
        }
        let p_flags = u32::from_le_bytes(elf_data[off + 4..off + 8].try_into().unwrap());
        let p_vaddr = u64::from_le_bytes(elf_data[off + 16..off + 24].try_into().unwrap());
        let p_filesz = u64::from_le_bytes(elf_data[off + 32..off + 40].try_into().unwrap());
        let p_memsz = u64::from_le_bytes(elf_data[off + 40..off + 48].try_into().unwrap());
        let p_align = u64::from_le_bytes(elf_data[off + 48..off + 56].try_into().unwrap());
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
        segs[nsegs as usize] = OnxSegment {
            vaddr: p_vaddr,
            filesz: p_filesz,
            memsz: p_memsz,
            offset: filesz_acc,
            flags,
            align: p_align as u32,
            reserved: 0,
        };
        filesz_acc = filesz_acc.saturating_add(p_filesz as u32);
        nsegs += 1;
    }

    let mut hdr_bytes = [0u8; 344];
    hdr_bytes[0..4].copy_from_slice(&ONX_MAGIC.to_le_bytes());
    hdr_bytes[4..8].copy_from_slice(&ONX_VERSION.to_le_bytes());
    hdr_bytes[8..16].copy_from_slice(&e_entry.to_le_bytes());
    hdr_bytes[16..20].copy_from_slice(&nsegs.to_le_bytes());
    let flags = if ring1 { ONX_FLAGS_RING1 } else { 0 };
    hdr_bytes[20..24].copy_from_slice(&flags.to_le_bytes());
    for (i, s) in segs.iter().enumerate() {
        let off = 24 + i * 40;
        hdr_bytes[off..off + 40].copy_from_slice(&s.to_bytes());
    }

    let mut out = File::create(&output).unwrap_or_else(|e| {
        eprintln!("create {}: {}", output, e);
        process::exit(1);
    });
    out.write_all(&hdr_bytes).unwrap();
    for i in 0..nsegs as usize {
        let p_offset = u64::from_le_bytes(
            elf_data[e_phoff + i * e_phentsize + 8..e_phoff + i * e_phentsize + 16]
                .try_into()
                .unwrap(),
        ) as usize;
        let p_filesz = u64::from_le_bytes(
            elf_data[e_phoff + i * e_phentsize + 32..e_phoff + i * e_phentsize + 40]
                .try_into()
                .unwrap(),
        ) as usize;
        if p_offset + p_filesz <= elf_data.len() {
            out.write_all(&elf_data[p_offset..p_offset + p_filesz])
                .unwrap();
        }
    }
    eprintln!(
        "elf2onx: {} -> {} (entry=0x{:x}, nsegs={}, ring={})",
        input,
        output,
        e_entry,
        nsegs,
        if ring1 { 1 } else { 2 }
    );
}
