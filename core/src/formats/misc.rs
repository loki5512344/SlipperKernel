/// FAT32 BPB (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fat32Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub fat_size: u32,
    pub root_cluster: u32,
}

impl Fat32Bpb {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 90 {
            return None;
        }
        if buf[510] != 0x55 || buf[511] != 0xAA {
            return None;
        }
        let bps = u16::from_le_bytes([buf[11], buf[12]]);
        if bps != 512 {
            return None;
        }
        let spc = buf[13];
        if spc == 0 || (spc & (spc - 1)) != 0 {
            return None;
        }
        let fat16_sz = u16::from_le_bytes([buf[22], buf[23]]) as u32;
        let fat_sz = if fat16_sz == 0 {
            u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]])
        } else {
            fat16_sz
        };
        Some(Self {
            bytes_per_sector: bps,
            sectors_per_cluster: spc,
            reserved_sectors: u16::from_le_bytes([buf[14], buf[15]]),
            num_fats: buf[16],
            fat_size: fat_sz,
            root_cluster: u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]),
        })
    }
}

// ─── 8.3 name helpers ────────────────────────────────────────────────────────

pub fn name_to_83(name: &[u8]) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let dot = name.iter().position(|&b| b == b'.');
    let (name_part, ext_part) = match dot {
        Some(i) => (&name[..i], Some(&name[i + 1..])),
        None => (name, None),
    };
    if name_part.len() > 8 || (ext_part.is_some() && ext_part.unwrap().len() > 3) {
        return None;
    }
    for (i, &b) in name_part.iter().enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    if let Some(ext) = ext_part {
        for (i, &b) in ext.iter().enumerate() {
            out[8 + i] = b.to_ascii_uppercase();
        }
    }
    Some(out)
}

pub fn matches_83(dirent11: &[u8], name: &[u8]) -> bool {
    let expected = match name_to_83(name) {
        Some(e) => e,
        None => return false,
    };
    for i in 0..11 {
        if !dirent11[i].eq_ignore_ascii_case(&expected[i]) {
            return false;
        }
    }
    true
}
