//! IPC channels — bidirectional ring buffers between a root service (owner)
//! and a user process (client).
//!
//! A root-space process creates a channel (`create`) and gets a channel ID.
//! A user process connects to it (`connect`); then either side may `send` /
//! `recv`. The buffer is a fixed 4 KiB ring; if it is full, `send` returns
//! `Errno::Busy` (MVP — no blocking).
use onyx_core::errno::{Errno, KResult};

const CHAN_MAX: usize = 32;
const CHAN_BUF_SIZE: usize = 4096;

#[derive(Clone, Copy)]
pub struct Channel {
    pub buf: [u8; CHAN_BUF_SIZE],
    pub head: u32,
    pub tail: u32,
    /// root service that created it.
    pub owner_pid: u32,
    /// user process that connected (0 until `connect` is called).
    pub client_pid: u32,
    pub used: bool,
    pub closed: bool,
}

static mut G_CHANNELS: [Channel; CHAN_MAX] = [Channel {
    buf: [0; CHAN_BUF_SIZE],
    head: 0,
    tail: 0,
    owner_pid: 0,
    client_pid: 0,
    used: false,
    closed: false,
}; CHAN_MAX];

/// Create a new channel owned by `owner_pid`. Returns the channel ID.
pub unsafe fn create(owner_pid: u32) -> KResult<u32> {
    let p = &raw mut G_CHANNELS;
    for i in 0..CHAN_MAX {
        if !(*p)[i].used {
            (*p)[i].used = true;
            (*p)[i].owner_pid = owner_pid;
            (*p)[i].client_pid = 0;
            (*p)[i].head = 0;
            (*p)[i].tail = 0;
            (*p)[i].closed = false;
            return Ok(i as u32);
        }
    }
    Err(Errno::NoMem)
}

/// Attach `client_pid` to an existing channel as the client endpoint.
pub unsafe fn connect(chan_id: u32, client_pid: u32) -> KResult<()> {
    let p = &raw mut G_CHANNELS;
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    if !(*p)[chan_id as usize].used {
        return Err(Errno::NoEnt);
    }
    (*p)[chan_id as usize].client_pid = client_pid;
    Ok(())
}

/// Write `len` bytes from `buf` into the channel ring buffer. Only the owner
/// or the client may send. Returns the number of bytes sent. If the buffer
/// cannot hold the entire message, returns `Errno::Busy` (MVP — no blocking).
pub unsafe fn send(chan_id: u32, buf: *const u8, len: u32) -> KResult<u32> {
    let p = &raw mut G_CHANNELS;
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut (*p)[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if cur_pid != ch.owner_pid && cur_pid != ch.client_pid {
        return Err(Errno::Perm);
    }

    let available = CHAN_BUF_SIZE as u32 - ch.tail.wrapping_sub(ch.head);
    if available < len {
        return Err(Errno::Busy);
    }

    let mut written = 0u32;
    while written < len {
        let idx = (ch.tail as usize) % CHAN_BUF_SIZE;
        ch.buf[idx] = *buf.add(written as usize);
        ch.tail = ch.tail.wrapping_add(1);
        written += 1;
    }
    Ok(written)
}

/// Read up to `len` bytes into `buf` from the channel ring buffer. Only the
/// owner or the client may recv. Returns the number of bytes read (0 if the
/// channel is empty — not an error).
pub unsafe fn recv(chan_id: u32, buf: *mut u8, len: u32) -> KResult<u32> {
    let p = &raw mut G_CHANNELS;
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut (*p)[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if cur_pid != ch.owner_pid && cur_pid != ch.client_pid {
        return Err(Errno::Perm);
    }

    let available = ch.tail.wrapping_sub(ch.head);
    if available == 0 {
        return Ok(0);
    }
    let to_read = len.min(available);

    let mut read = 0u32;
    while read < to_read {
        let idx = (ch.head as usize) % CHAN_BUF_SIZE;
        *buf.add(read as usize) = ch.buf[idx];
        ch.head = ch.head.wrapping_add(1);
        read += 1;
    }
    Ok(read)
}

/// Close and free a channel.
pub unsafe fn close(chan_id: u32) -> KResult<()> {
    let p = &raw mut G_CHANNELS;
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    (*p)[chan_id as usize].closed = true;
    (*p)[chan_id as usize].used = false;
    Ok(())
}
