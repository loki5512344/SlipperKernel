//! Format trait + vformat engine.

#![allow(unused_imports)]

use super::format_num::{write_dec, write_hex_or_dec};
use crate::parser::{le32, le64};

pub trait Write {
    fn write_str(&mut self, s: &str);
    fn write_char(&mut self, c: u8) {
        self.write_str(core::str::from_utf8(&[c]).unwrap_or("?"));
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FmtSpec {
    pub width: usize,
    pub zero_pad: bool,
}

fn parse_num(buf: &[u8]) -> (usize, usize) {
    let mut v = 0usize;
    let mut i = 0;
    while i < buf.len() && buf[i].is_ascii_digit() {
        v = v
            .saturating_mul(10)
            .saturating_add((buf[i] - b'0') as usize);
        i += 1;
    }
    (v, i)
}

pub fn vformat<W: Write>(out: &mut W, fmt: &str, args: &[super::Arg]) {
    let bytes = fmt.as_bytes();
    let mut i = 0;
    let mut arg_idx = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != b'%' {
            out.write_char(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            out.write_str("%");
            break;
        }
        let mut spec = FmtSpec {
            width: 0,
            zero_pad: false,
        };
        if bytes[i] == b'0' {
            spec.zero_pad = true;
            i += 1;
        }
        let (w, consumed) = parse_num(&bytes[i..]);
        spec.width = w;
        i += consumed;
        while i < bytes.len() && (bytes[i] == b'l' || bytes[i] == b'z') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let conv = bytes[i];
        i += 1;
        let arg = if arg_idx < args.len() {
            Some(&args[arg_idx])
        } else {
            None
        };
        match conv {
            b'%' => out.write_char(b'%'),
            b's' => {
                if let Some(super::Arg::Str(s)) = arg {
                    out.write_str(s);
                } else if let Some(super::Arg::CStr(ptr)) = arg {
                    unsafe {
                        let mut p = *ptr;
                        while *p != 0 {
                            out.write_char(*p);
                            p = p.add(1);
                        }
                    }
                }
                arg_idx += 1;
            }
            b'c' => {
                if let Some(super::Arg::Char(c)) = arg {
                    out.write_char(*c);
                }
                arg_idx += 1;
            }
            b'd' => {
                if let Some(super::Arg::I64(v)) = arg {
                    write_dec(out, *v, spec);
                } else if let Some(super::Arg::U64(v)) = arg {
                    write_dec(out, *v as i64, spec);
                } else if let Some(super::Arg::USize(v)) = arg {
                    write_dec(out, *v as i64, spec);
                } else if let Some(super::Arg::ISize(v)) = arg {
                    write_dec(out, *v as i64, spec);
                }
                arg_idx += 1;
            }
            b'u' => {
                if let Some(super::Arg::U64(v)) = arg {
                    write_hex_or_dec(out, *v, spec, false);
                } else if let Some(super::Arg::USize(v)) = arg {
                    write_hex_or_dec(out, *v as u64, spec, false);
                }
                arg_idx += 1;
            }
            b'x' => {
                if let Some(super::Arg::U64(v)) = arg {
                    write_hex_or_dec(out, *v, spec, true);
                } else if let Some(super::Arg::USize(v)) = arg {
                    write_hex_or_dec(out, *v as u64, spec, true);
                }
                arg_idx += 1;
            }
            b'p' => {
                out.write_str("0x");
                let pv = match arg {
                    Some(super::Arg::U64(v)) => Some(*v),
                    Some(super::Arg::USize(v)) => Some(*v as u64),
                    _ => None,
                };
                if let Some(v) = pv {
                    write_hex_or_dec(
                        out,
                        v,
                        FmtSpec {
                            width: 16,
                            zero_pad: true,
                        },
                        true,
                    );
                }
                arg_idx += 1;
            }
            _ => {
                out.write_char(b'%');
                out.write_char(conv);
            }
        }
    }
}
