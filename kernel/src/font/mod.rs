//! PSF1/PSF2 font loader with Unicode table support.
//!
//! Loads glyph bitmaps from PSF1/PSF2 font files and parses the optional
//! Unicode table to map Unicode codepoints → glyph indices. This allows
//! the framebuffer to render non-ASCII characters (e.g. Cyrillic).

use onyx_core::errno::{Errno, KResult};

pub const FONT_W: usize = 8;
pub const FONT_H: usize = 16;
pub const FONT_NUM_GLYPHS: usize = 256;
pub const FONT_GLYPH_BYTES: usize = FONT_H;

static mut G_FONT: Option<PcfFont> = None;

#[derive(Clone, Copy)]
pub struct PcfFont {
    pub width: u32,
    pub height: u32,
    pub charsize: u32,
    pub num_glyphs: u32,
    pub glyphs: *const u8,
    pub unicode: *const u8,
    pub unicode_len: usize,
}

// ── Unicode → glyph mapping table ──────────────────────────────────────

const UNICODE_MAP_SIZE: usize = 512;

#[derive(Clone, Copy)]
struct UniMapEntry {
    codepoint: u32,
    glyph_idx: u32,
}

static mut G_UNI_MAP: [UniMapEntry; UNICODE_MAP_SIZE] =
    [UniMapEntry { codepoint: 0, glyph_idx: 0 }; UNICODE_MAP_SIZE];
static mut G_UNI_MAP_LEN: usize = 0;

/// Insert a codepoint→glyph mapping into the Unicode table.
unsafe fn uni_map_insert(cp: u32, idx: u32) {
    if G_UNI_MAP_LEN < UNICODE_MAP_SIZE {
        G_UNI_MAP[G_UNI_MAP_LEN] = UniMapEntry {
            codepoint: cp,
            glyph_idx: idx,
        };
        G_UNI_MAP_LEN += 1;
    }
}

// ── Public API ──────────────────────────────────────────────────────────

pub fn font() -> Option<PcfFont> {
    unsafe { G_FONT }
}

/// Return the actual font height from the loaded font (falls back to FONT_H).
pub fn font_height() -> usize {
    unsafe { G_FONT.map(|f| f.height as usize).unwrap_or(FONT_H) }
}

/// Return the actual font width from the loaded font (falls back to FONT_W).
pub fn font_width() -> usize {
    unsafe { G_FONT.map(|f| f.width as usize).unwrap_or(FONT_W) }
}

/// Return the charsize (bytes per glyph) from the loaded font.
pub fn font_charsize() -> usize {
    unsafe { G_FONT.map(|f| f.charsize as usize).unwrap_or(FONT_GLYPH_BYTES) }
}

pub unsafe fn init(data: &[u8]) -> KResult<()> {
    if data.len() < 4 {
        return Err(Errno::Io);
    }
    let magic = u32::from_le_bytes(data[..4].try_into().unwrap());
    if magic == 0x0436 || (magic & 0xFFFF) == 0x0436 {
        init_psf1(data)
    } else if magic == 0x864ab572 {
        init_psf2(data)
    } else {
        Err(Errno::NoEnt)
    }
}

// ── PSF1 ────────────────────────────────────────────────────────────────

unsafe fn init_psf1(data: &[u8]) -> KResult<()> {
    if data.len() < 4 {
        return Err(Errno::Io);
    }
    let magic = u16::from_le_bytes(data[..2].try_into().unwrap());
    if magic != 0x0436 {
        return Err(Errno::NoEnt);
    }
    let mode = data[2];
    let charsize = data[3] as u32;
    let num_glyphs: u32 = if mode & 0x01 != 0 { 512 } else { 256 };
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    if data.len() < 4 + glyph_bytes {
        return Err(Errno::Io);
    }
    let (unicode_ptr, unicode_len) = if mode & 0x02 != 0 && data.len() > 4 + glyph_bytes {
        let ustart = 4 + glyph_bytes;
        (data.as_ptr().add(ustart), data.len() - ustart)
    } else {
        (core::ptr::null(), 0)
    };
    G_FONT = Some(PcfFont {
        width: 8,
        height: charsize,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(4),
        unicode: unicode_ptr,
        unicode_len,
    });

    // Parse PSF1 Unicode table if present (mode bit 1)
    if mode & 0x02 != 0 {
        parse_psf1_unicode_table(data, 4, num_glyphs, charsize);
    }

    Ok(())
}

/// Parse PSF1 Unicode table.
///
/// Format: after glyph data, sequences of UCS-2 values (2 bytes LE) for each
/// glyph in order. 0xFFFF separates glyphs. 0xFFFE separates multiple Unicode
/// values for the same glyph.
unsafe fn parse_psf1_unicode_table(data: &[u8], hdr_size: usize, num_glyphs: u32, charsize: u32) {
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let table_start = hdr_size + glyph_bytes;
    if table_start + 2 > data.len() {
        return;
    }
    let table = &data[table_start..];
    let mut glyph_idx = 0u32;
    let mut i = 0usize;

    while i + 1 < table.len() && glyph_idx < num_glyphs {
        let lo = table[i] as u16;
        let hi = table[i + 1] as u16;
        let val = (hi << 8) | lo;
        i += 2;

        if val == 0xFFFF {
            // End of this glyph's entries; advance to next glyph
            glyph_idx += 1;
            continue;
        }
        if val == 0xFFFE {
            // Separator — multiple Unicode values for same glyph; skip
            continue;
        }
        // Map this codepoint to the current glyph index
        let cp = val as u32;
        if cp >= 256 {
            uni_map_insert(cp, glyph_idx);
        }
    }
}

// ── PSF2 ────────────────────────────────────────────────────────────────

/// PSF2_HAS_UNICODE_TABLE flag (bit 0 of the `flags` field).
const PSF2_HAS_UNICODE_TABLE: u32 = 1;

unsafe fn init_psf2(data: &[u8]) -> KResult<()> {
    if data.len() < 32 {
        return Err(Errno::Io);
    }
    let _version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let hdr_size = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let flags = u32::from_le_bytes(data[12..16].try_into().unwrap());
    let num_glyphs = u32::from_le_bytes(data[16..20].try_into().unwrap());
    let charsize = u32::from_le_bytes(data[20..24].try_into().unwrap());
    let height = u32::from_le_bytes(data[24..28].try_into().unwrap());
    let width = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let end = hdr_size + glyph_bytes;
    if data.len() < end {
        return Err(Errno::Io);
    }
    let (unicode_ptr, unicode_len) = if data.len() > end {
        (data.as_ptr().add(end), data.len() - end)
    } else {
        (core::ptr::null(), 0)
    };
    G_FONT = Some(PcfFont {
        width,
        height,
        charsize,
        num_glyphs,
        glyphs: data.as_ptr().add(hdr_size),
        unicode: unicode_ptr,
        unicode_len,
    });

    // Parse PSF2 Unicode table if present
    if flags & PSF2_HAS_UNICODE_TABLE != 0 {
        parse_psf2_unicode_table(data, hdr_size, num_glyphs, charsize);
    }

    Ok(())
}

/// Parse PSF2 Unicode table.
///
/// Format: after all glyph data, sequences of UTF-8 strings (null-terminated)
/// for each glyph. 0xFF separates entries for different glyphs. 0xFE separates
/// multiple strings that map to the same glyph.
unsafe fn parse_psf2_unicode_table(data: &[u8], hdr_size: usize, num_glyphs: u32, charsize: u32) {
    let glyph_bytes = (num_glyphs as usize) * (charsize as usize);
    let table_start = hdr_size + glyph_bytes;
    if table_start >= data.len() {
        return;
    }
    let table = &data[table_start..];
    let mut glyph_idx = 0u32;
    let mut i = 0usize;

    while i < table.len() && glyph_idx < num_glyphs {
        let b = table[i];

        if b == 0xFF {
            // Separator — advance to next glyph
            glyph_idx += 1;
            i += 1;
            continue;
        }

        if b == 0xFE {
            // Same glyph, additional Unicode mapping
            i += 1;
            continue;
        }

        // Decode a UTF-8 codepoint
        let cp = decode_utf8(table, &mut i);
        if cp != 0 && cp >= 256 {
            uni_map_insert(cp, glyph_idx);
        }

        // Skip remaining bytes of this UTF-8 string until separator/null
        while i < table.len() && table[i] != 0xFF && table[i] != 0xFE && table[i] != 0 {
            i += 1;
        }
        // Skip null terminator
        if i < table.len() && table[i] == 0 {
            i += 1;
        }
    }
}

/// Decode a single UTF-8 codepoint from `data` starting at `pos`, advancing `pos`.
unsafe fn decode_utf8(data: &[u8], pos: &mut usize) -> u32 {
    if *pos >= data.len() {
        return 0;
    }
    let b0 = data[*pos];
    if b0 < 0x80 {
        *pos += 1;
        return b0 as u32;
    }
    // Multi-byte UTF-8
    let (mask, n) = if b0 < 0xE0 {
        (0x1Fu8, 2)
    } else if b0 < 0xF0 {
        (0x0F, 3)
    } else {
        (0x07, 4)
    };
    let mut cp = (b0 & mask) as u32;
    for _ in 1..n {
        *pos += 1;
        if *pos >= data.len() {
            return 0;
        }
        cp = (cp << 6) | ((data[*pos] & 0x3F) as u32);
    }
    *pos += 1;
    cp
}

// ── Glyph lookup ────────────────────────────────────────────────────────

/// Look up the glyph index for a Unicode codepoint.
/// Returns `Some(idx)` if found, `None` otherwise.
/// Codepoints 0–255 use direct mapping (index = codepoint) if within range.
pub fn glyph_for_unicode(cp: u32) -> Option<u32> {
    if cp < 256 {
        // ASCII and Latin-1 — direct mapping
        let f = unsafe { G_FONT? };
        if cp < f.num_glyphs {
            return Some(cp);
        }
    }
    // Linear scan through the Unicode map
    unsafe {
        for i in 0..G_UNI_MAP_LEN {
            if G_UNI_MAP[i].codepoint == cp {
                return Some(G_UNI_MAP[i].glyph_idx);
            }
        }
    }
    None
}

/// Get glyph bitmap for a byte (ASCII/Latin-1). Returns a fixed 16-byte array.
pub fn glyph_bitmap(c: u8) -> &'static [u8; FONT_GLYPH_BYTES] {
    unsafe {
        if let Some(f) = G_FONT {
            let idx = (c as u32).min(f.num_glyphs - 1) as usize;
            let off = idx * f.charsize as usize;
            let ptr = f.glyphs.add(off) as *const [u8; FONT_GLYPH_BYTES];
            &*ptr
        } else {
            &BLANK_GLYPH
        }
    }
}

#[derive(Clone, Copy)]
pub struct GlyphData {
    pub data: *const u8,
    pub charsize: u32,
    pub width: u32,
    pub height: u32,
}

pub fn glyph_bitmap_unicode(cp: u32) -> GlyphData {
    if let Some(idx) = glyph_for_unicode(cp) {
        unsafe {
            if let Some(f) = G_FONT {
                let safe_idx = (idx as usize).min(f.num_glyphs as usize - 1);
                let off = safe_idx * f.charsize as usize;
                return GlyphData {
                    data: f.glyphs.add(off),
                    charsize: f.charsize,
                    width: f.width,
                    height: f.height,
                };
            }
        }
    }
    unsafe {
        if let Some(f) = G_FONT {
            let off = (b'?' as usize).min(f.num_glyphs as usize - 1) * f.charsize as usize;
            GlyphData {
                data: f.glyphs.add(off),
                charsize: f.charsize,
                width: f.width,
                height: f.height,
            }
        } else {
            GlyphData {
                data: BLANK_GLYPH.as_ptr(),
                charsize: FONT_GLYPH_BYTES as u32,
                width: FONT_W as u32,
                height: FONT_H as u32,
            }
        }
    }
}

pub fn glyph_for_cp(cp: u32) -> Option<u8> {
    unsafe {
        let f = G_FONT?;
        if f.unicode.is_null() || f.unicode_len == 0 {
            return (cp as u8 <= 0x7F || (f.num_glyphs > 256 && cp < 256)).then(|| cp as u8);
        }
        let mut pos = 0usize;
        let mut glyph: u32 = 0;
        while pos + 1 < f.unicode_len && glyph < f.num_glyphs {
            let val = u16::from_le_bytes([*f.unicode.add(pos), *f.unicode.add(pos + 1)]);
            pos += 2;
            if val == 0xFFFF {
                glyph += 1;
            } else if val == 0xFFFE {
            } else if val as u32 == cp {
                return Some(glyph as u8);
            }
        }
        None
    }
}

pub fn glyph_or_default(cp: u32) -> u8 {
    glyph_for_cp(cp).unwrap_or_else(|| {
        if cp < 256 { cp as u8 } else { b'?' }
    })
}

static BLANK_GLYPH: [u8; FONT_GLYPH_BYTES] = [0u8; FONT_GLYPH_BYTES];
