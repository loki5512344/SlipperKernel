//! RLE compression used by the snapshot subsystem.
//!
//! Packet format (each packet starts with a tag byte):
//! - `tag & 0x80 != 0` → run packet: count = `(tag & 0x7F) + 1` (1..128),
//!   next byte = value, expand to `count × value`.
//! - `tag & 0x80 == 0` → literal packet: count = `tag + 1` (1..128),
//!   followed by `count` literal bytes.
//!
//! Runs of >= 3 identical bytes are encoded as a run packet; everything else
//! is grouped into literal packets of up to 128 bytes each. Worst-case
//! expansion for incompressible input is ~N + N/128 bytes.

/// RLE-compress `src` into `dst`. Returns the compressed size, or 0 on
/// overflow (`dst` too small). Caller must ensure `dst` is at least
/// `src.len() + src.len()/128 + 2` bytes for incompressible input.
pub(super) unsafe fn rle_compress(src: &[u8], dst: &mut [u8]) -> usize {
    let n = src.len();
    let mut i: usize = 0;
    let mut out: usize = 0;
    while i < n {
        let cur = src[i];
        // Count run length (max 128).
        let mut run: usize = 1;
        while i + run < n && src[i + run] == cur && run < 128 {
            run += 1;
        }
        if run >= 3 {
            if out + 2 > dst.len() {
                return 0;
            }
            dst[out] = 0x80 | ((run - 1) as u8);
            dst[out + 1] = cur;
            out += 2;
            i += run;
        } else {
            // Collect literal bytes (up to 128), stopping at a 3+ run.
            let lit_start = i;
            let mut lit_len: usize = 0;
            while i + lit_len < n && lit_len < 128 {
                let b = src[i + lit_len];
                let mut k: usize = 0;
                while i + lit_len + k < n && src[i + lit_len + k] == b && k < 3 {
                    k += 1;
                }
                if k >= 3 {
                    break;
                }
                lit_len += 1;
            }
            if lit_len == 0 {
                lit_len = 1;
            }
            if out + 1 + lit_len > dst.len() {
                return 0;
            }
            dst[out] = (lit_len - 1) as u8;
            for j in 0..lit_len {
                dst[out + 1 + j] = src[lit_start + j];
            }
            out += 1 + lit_len;
            i += lit_len;
        }
    }
    out
}

/// RLE-decompress `src` into `dst`. Returns the number of bytes written, or 0
/// on overflow / truncated input.
pub(super) unsafe fn rle_decompress(src: &[u8], dst: &mut [u8]) -> usize {
    let mut i: usize = 0;
    let mut out: usize = 0;
    while i < src.len() && out < dst.len() {
        let tag = src[i];
        i += 1;
        if tag & 0x80 != 0 {
            let count = ((tag & 0x7F) as usize) + 1;
            if i >= src.len() || out + count > dst.len() {
                return 0;
            }
            let val = src[i];
            i += 1;
            for j in 0..count {
                dst[out + j] = val;
            }
            out += count;
        } else {
            let count = (tag as usize) + 1;
            if i + count > src.len() || out + count > dst.len() {
                return 0;
            }
            for j in 0..count {
                dst[out + j] = src[i + j];
            }
            i += count;
            out += count;
        }
    }
    out
}
