#![allow(dead_code)]
use crate::syscalls;

pub const PASSWD_PATH: &[u8] = b"/etc/passwd";
pub const SHADOW_PATH: &[u8] = b"/etc/shadow";
pub const MAX_USERS: usize = 16;
pub const MAX_LINE: usize = 256;

#[derive(Clone, Copy)]
pub struct PasswdEntry {
    pub name: [u8; 32],
    pub uid: u32,
    pub gid: u32,
    pub home: [u8; 64],
    pub shell: [u8; 32],
}

pub fn parse_passwd(data: &[u8], users: &mut [PasswdEntry; MAX_USERS]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos < data.len() && count < MAX_USERS {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        if line.is_empty() || line[0] == b'#' {
            continue;
        }

        let mut fields = [0usize; 5];
        let mut fi = 0;
        let mut start = 0;
        for (i, &b) in line.iter().enumerate() {
            if b == b':' {
                if fi < fields.len() {
                    fields[fi] = start;
                    fi += 1;
                }
                start = i + 1;
            }
        }
        fields[4] = start;

        if fi < 4 {
            continue;
        }

        let name = &line[fields[0]..fields[1]];
        let uid_str = &line[fields[1]..fields[2]];
        let gid_str = &line[fields[2]..fields[3]];
        let home = &line[fields[3]..fields[4]];
        let shell = &line[fields[4]..];

        let uid = parse_dec(uid_str);
        let gid = parse_dec(gid_str);

        let mut entry = PasswdEntry {
            name: [0; 32],
            uid,
            gid,
            home: [0; 64],
            shell: [0; 32],
        };
        copy_slice(&mut entry.name, name);
        copy_slice(&mut entry.home, home);
        copy_slice(&mut entry.shell, shell);
        users[count] = entry;
        count += 1;
    }
    count
}

pub fn find_user(users: &[PasswdEntry; MAX_USERS], count: usize, name: &[u8]) -> Option<usize> {
    users[..count].iter().position(|entry| {
        let mut match_len = 0;
        while match_len < entry.name.len() && entry.name[match_len] != 0 && match_len < name.len()
        {
            if entry.name[match_len] != name[match_len] {
                break;
            }
            match_len += 1;
        }
        match_len == name.len() && (entry.name[match_len] == 0 || match_len == entry.name.len())
    })
}

pub fn find_user_by_uid(users: &[PasswdEntry; MAX_USERS], count: usize, uid: u32) -> Option<usize> {
    users[..count].iter().position(|e| e.uid == uid)
}

pub fn read_passwd(users: &mut [PasswdEntry; MAX_USERS]) -> Result<usize, i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);
    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { syscalls::close(fd as u64) };
    Ok(parse_passwd(&buf[..total], users))
}

pub fn read_shadow_password(username: &[u8]) -> Result<[u8; 64], i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);
    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    loop {
        let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { syscalls::close(fd as u64) };

    let mut password = [0u8; 64];
    let data = &buf[..total];
    let mut pos = 0;
    while pos < data.len() {
        let line_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(n) => pos + n,
            None => data.len(),
        };
        let line = &data[pos..line_end];
        pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];
        let pass = &line[colon + 1..];

        if name.len() == username.len() && name == username {
            let n = pass.len().min(63);
            password[..n].copy_from_slice(&pass[..n]);
            return Ok(password);
        }
    }
    Err(-2)
}

pub fn update_shadow_password(username: &[u8], new_password: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd >= 0 {
        loop {
            let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
            if n <= 0 {
                break;
            }
            total += n as usize;
            if total >= buf.len() {
                break;
            }
        }
        unsafe { syscalls::close(fd as u64) };
    }

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut found = false;
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => {
                let copy_end = (out_pos + line.len()).min(out.len());
                let to_copy = copy_end - out_pos;
                out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
                out_pos = copy_end;
                if out_pos < out.len() {
                    out[out_pos] = b'\n';
                    out_pos += 1;
                }
                continue;
            }
        };
        let name = &line[..colon];

        if name == username {
            let entry = format_shadow_entry(username, new_password);
            let copy_end = (out_pos + entry.len()).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
            found = true;
        } else {
            let copy_end = (out_pos + line.len()).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
        }
    }

    if !found {
        let entry = format_shadow_entry(username, new_password);
        let copy_end = (out_pos + entry.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o600, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

pub fn update_passwd_entry(username: &[u8], uid: u32, gid: u32, home: &[u8], shell: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd >= 0 {
        loop {
            let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
            if n <= 0 {
                break;
            }
            total += n as usize;
            if total >= buf.len() {
                break;
            }
        }
        unsafe { syscalls::close(fd as u64) };
    }

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut found = false;
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];

        if name == username {
            let entry = format_passwd_entry(username, uid, gid, home, shell);
            let copy_end = (out_pos + entry.len()).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
            found = true;
        } else {
            let copy_end = (out_pos + line.len()).min(out.len());
            let to_copy = copy_end - out_pos;
            out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
            out_pos = copy_end;
            if out_pos < out.len() {
                out[out_pos] = b'\n';
                out_pos += 1;
            }
        }
    }

    if !found {
        let entry = format_passwd_entry(username, uid, gid, home, shell);
        let copy_end = (out_pos + entry.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&entry[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o644, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

pub fn delete_passwd_entry(username: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = PASSWD_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&PASSWD_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    loop {
        let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { syscalls::close(fd as u64) };

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];

        if name == username {
            continue;
        }

        let copy_end = (out_pos + line.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o644, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

pub fn delete_shadow_entry(username: &[u8]) -> Result<(), i64> {
    let mut path_buf = [0u8; 64];
    let n = SHADOW_PATH.len().min(63);
    path_buf[..n].copy_from_slice(&SHADOW_PATH[..n]);

    let mut buf = [0u8; 4096];
    let mut total = 0usize;

    let fd = unsafe { syscalls::open(path_buf.as_ptr(), 0, 0) };
    if fd < 0 {
        return Err(fd);
    }
    loop {
        let n = unsafe { syscalls::read(fd as u64, buf[total..].as_mut_ptr(), (buf.len() - total) as u64) };
        if n <= 0 {
            break;
        }
        total += n as usize;
        if total >= buf.len() {
            break;
        }
    }
    unsafe { syscalls::close(fd as u64) };

    let mut out = [0u8; 4096];
    let mut out_pos = 0;
    let data = &buf[..total];
    let mut data_pos = 0;

    while data_pos < data.len() {
        let line_end = match data[data_pos..].iter().position(|&b| b == b'\n') {
            Some(n) => data_pos + n,
            None => data.len(),
        };
        let line = &data[data_pos..line_end];
        data_pos = line_end + 1;

        let colon = match line.iter().position(|&b| b == b':') {
            Some(n) => n,
            None => continue,
        };
        let name = &line[..colon];

        if name == username {
            continue;
        }

        let copy_end = (out_pos + line.len()).min(out.len());
        let to_copy = copy_end - out_pos;
        out[out_pos..copy_end].copy_from_slice(&line[..to_copy]);
        out_pos = copy_end;
        if out_pos < out.len() {
            out[out_pos] = b'\n';
            out_pos += 1;
        }
    }

    let fd = unsafe { syscalls::create(path_buf.as_ptr(), 0o600, 0) };
    if fd < 0 {
        return Err(fd);
    }
    let _ = unsafe { syscalls::write_fd(fd as u64, out.as_ptr(), out_pos) };
    unsafe { syscalls::close(fd as u64) };
    Ok(())
}

fn format_shadow_entry(username: &[u8], password: &[u8]) -> [u8; 128] {
    let mut buf = [0u8; 128];
    let mut pos = 0;
    for &b in username.iter() {
        if pos >= buf.len() { break; }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = b':';
        pos += 1;
    }
    for &b in password.iter() {
        if pos >= buf.len() { break; }
        buf[pos] = b;
        pos += 1;
    }
    buf
}

fn format_passwd_entry(username: &[u8], uid: u32, gid: u32, home: &[u8], shell: &[u8]) -> [u8; 256] {
    let mut buf = [0u8; 256];
    let mut pos = 0;

    for &b in username.iter() {
        if pos >= buf.len() { break; }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() { buf[pos] = b':'; pos += 1; }

    let uid_str = format_dec(uid);
    for &b in uid_str.iter() {
        if pos >= buf.len() || b == 0 { break; }
        if b == 0 { break; }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() { buf[pos] = b':'; pos += 1; }

    let gid_str = format_dec(gid);
    for &b in gid_str.iter() {
        if pos >= buf.len() || b == 0 { break; }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() { buf[pos] = b':'; pos += 1; }

    for &b in home.iter() {
        if pos >= buf.len() { break; }
        buf[pos] = b;
        pos += 1;
    }
    if pos < buf.len() { buf[pos] = b':'; pos += 1; }

    for &b in shell.iter() {
        if pos >= buf.len() { break; }
        buf[pos] = b;
        pos += 1;
    }
    buf
}

fn format_dec(n: u32) -> [u8; 12] {
    let mut buf = [0u8; 12];
    let mut pos = 11;
    if n == 0 {
        buf[10] = b'0';
        return buf;
    }
    let mut val = n;
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    buf
}

fn parse_dec(s: &[u8]) -> u32 {
    let mut val: u32 = 0;
    for &b in s.iter() {
        if b.is_ascii_digit() {
            val = val.wrapping_mul(10).wrapping_add(u32::from(b - b'0'));
        } else {
            break;
        }
    }
    val
}

fn copy_slice(dst: &mut [u8], src: &[u8]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}
