use crate::arch::trap_frame::TrapFrame;
use onyx_core::errno::{Errno, KResult};

const CHAN_MAX: usize = 32;
const CHAN_BUF_SIZE: usize = 4096;
const CHAN_NAME_MAX: usize = 32;
const CHAN_MAX_CLIENTS: usize = 4;

#[derive(Clone, Copy)]
pub struct Channel {
    pub buf: [u8; CHAN_BUF_SIZE],
    pub head: u32,
    pub tail: u32,
    pub owner_pid: u32,
    pub clients: [u32; CHAN_MAX_CLIENTS],
    pub num_clients: u32,
    pub name: [u8; CHAN_NAME_MAX],
    pub name_len: u8,
    pub used: bool,
    pub closed: bool,
    pub send_wait: *mut crate::proc::Proc,
    pub recv_wait: *mut crate::proc::Proc,
}

static mut G_CHANNELS: [Channel; CHAN_MAX] = [Channel {
    buf: [0; CHAN_BUF_SIZE],
    head: 0,
    tail: 0,
    owner_pid: 0,
    clients: [0; CHAN_MAX_CLIENTS],
    num_clients: 0,
    name: [0; CHAN_NAME_MAX],
    name_len: 0,
    used: false,
    closed: false,
    send_wait: core::ptr::null_mut(),
    recv_wait: core::ptr::null_mut(),
}; CHAN_MAX];

fn pid_allowed(ch: &Channel, pid: u32) -> bool {
    if pid == ch.owner_pid {
        return true;
    }
    for &c in ch.clients[..ch.num_clients as usize].iter() {
        if c == pid {
            return true;
        }
    }
    false
}

/// Create an anonymous channel owned by `owner_pid`.
pub unsafe fn create(owner_pid: u32) -> KResult<u32> {
    for i in 0..CHAN_MAX {
        if !G_CHANNELS[i].used {
            G_CHANNELS[i] = Channel {
                buf: [0; CHAN_BUF_SIZE],
                head: 0,
                tail: 0,
                owner_pid,
                clients: [0; CHAN_MAX_CLIENTS],
                num_clients: 0,
                name: [0; CHAN_NAME_MAX],
                name_len: 0,
                used: true,
                closed: false,
                send_wait: core::ptr::null_mut(),
                recv_wait: core::ptr::null_mut(),
            };
            return Ok(i as u32);
        }
    }
    Err(Errno::NoMem)
}

/// Create a named channel. Name lookup is case-sensitive.
pub unsafe fn create_named(name: &[u8], owner_pid: u32) -> KResult<u32> {
    if name.is_empty() || name.len() > CHAN_NAME_MAX - 1 {
        return Err(Errno::Inval);
    }
    // Check for duplicate name.
    if find_by_name(name).is_some() {
        return Err(Errno::Exist);
    }
    let id = create(owner_pid)?;
    let ch = &mut G_CHANNELS[id as usize];
    let nlen = name.len().min(CHAN_NAME_MAX - 1);
    ch.name[..nlen].copy_from_slice(&name[..nlen]);
    ch.name_len = nlen as u8;
    Ok(id)
}

/// Find a channel by name. Returns the channel ID if found.
pub unsafe fn find_by_name(name: &[u8]) -> Option<u32> {
    for i in 0..CHAN_MAX {
        let ch = &G_CHANNELS[i];
        if ch.used && ch.name_len as usize == name.len() && &ch.name[..ch.name_len as usize] == name {
            return Some(i as u32);
        }
    }
    None
}

/// Open a named channel: find it and connect the calling process as a client.
pub unsafe fn open_by_name(name: &[u8], client_pid: u32) -> KResult<u32> {
    let id = find_by_name(name).ok_or(Errno::NoEnt)?;
    let ch = &mut G_CHANNELS[id as usize];
    if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
        return Err(Errno::NoMem);
    }
    // Check for duplicate connection.
    for &c in ch.clients[..ch.num_clients as usize].iter() {
        if c == client_pid {
            return Ok(id); // Already connected
        }
    }
    ch.clients[ch.num_clients as usize] = client_pid;
    ch.num_clients += 1;
    Ok(id)
}

/// Disconnect a client from a channel.
pub unsafe fn disconnect(chan_id: u32, pid: u32) {
    if chan_id as usize >= CHAN_MAX {
        return;
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return;
    }
    for i in 0..ch.num_clients as usize {
        if ch.clients[i] == pid {
            ch.clients[i] = ch.clients[ch.num_clients as usize - 1];
            ch.num_clients -= 1;
            return;
        }
    }
}

/// Attach `client_pid` to an existing channel by numeric ID.
pub unsafe fn connect(chan_id: u32, client_pid: u32) -> KResult<()> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return Err(Errno::NoEnt);
    }
    if ch.num_clients as usize >= CHAN_MAX_CLIENTS {
        return Err(Errno::NoMem);
    }
    ch.clients[ch.num_clients as usize] = client_pid;
    ch.num_clients += 1;
    Ok(())
}

unsafe fn wait_enqueue(wait_head: &mut *mut crate::proc::Proc) {
    let p = crate::proc::current() as *mut crate::proc::Proc;
    (*p).state = crate::proc::ProcState::Waiting;
    (*p).wait_next = *wait_head;
    *wait_head = p;
}

unsafe fn wait_wake_all(wait_head: &mut *mut crate::proc::Proc) {
    let mut cur = *wait_head;
    while !cur.is_null() {
        let next = (*cur).wait_next;
        (*cur).state = crate::proc::ProcState::Ready;
        (*cur).wait_next = core::ptr::null_mut();
        cur = next;
    }
    *wait_head = core::ptr::null_mut();
}

pub unsafe fn send(
    chan_id: u32,
    buf: *const u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if !pid_allowed(ch, cur_pid) {
        return Err(Errno::Perm);
    }

    let available = CHAN_BUF_SIZE as u32 - ch.tail.wrapping_sub(ch.head);
    if available < len {
        if let Some(tf) = tf {
            wait_enqueue(&mut ch.send_wait);
            crate::proc::scheduler::set_need_resched(true);
            crate::proc::scheduler::sched_yield(tf);
            return Err(Errno::Busy);
        }
        return Err(Errno::Busy);
    }

    let mut written = 0u32;
    while written < len {
        let idx = (ch.tail as usize) % CHAN_BUF_SIZE;
        ch.buf[idx] = *buf.add(written as usize);
        ch.tail = ch.tail.wrapping_add(1);
        written += 1;
    }

    if !ch.recv_wait.is_null() {
        wait_wake_all(&mut ch.recv_wait);
        crate::proc::scheduler::set_need_resched(true);
    }
    Ok(written)
}

pub unsafe fn recv(
    chan_id: u32,
    buf: *mut u8,
    len: u32,
    tf: Option<&mut TrapFrame>,
) -> KResult<u32> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used || ch.closed {
        return Err(Errno::Pipe);
    }
    let cur_pid = crate::proc::current_pid();
    if !pid_allowed(ch, cur_pid) {
        return Err(Errno::Perm);
    }

    let available = ch.tail.wrapping_sub(ch.head);
    if available == 0 {
        if let Some(tf) = tf {
            wait_enqueue(&mut ch.recv_wait);
            crate::proc::scheduler::set_need_resched(true);
            crate::proc::scheduler::sched_yield(tf);
            return Ok(0);
        }
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

    if !ch.send_wait.is_null() {
        wait_wake_all(&mut ch.send_wait);
        crate::proc::scheduler::set_need_resched(true);
    }
    Ok(read)
}

pub unsafe fn close(chan_id: u32) -> KResult<()> {
    if chan_id as usize >= CHAN_MAX {
        return Err(Errno::Inval);
    }
    let ch = &mut G_CHANNELS[chan_id as usize];
    if !ch.used {
        return Err(Errno::NoEnt);
    }
    ch.closed = true;
    ch.used = false;
    Ok(())
}

/// Return number of named channels (for ipcfs readdir).
pub unsafe fn named_count() -> u32 {
    let mut n = 0;
    for i in 0..CHAN_MAX {
        if G_CHANNELS[i].used && G_CHANNELS[i].name_len > 0 {
            n += 1;
        }
    }
    n
}

/// Get the name of a channel by index in the named-channel list (for readdir).
/// Returns `None` when there are no more names for the given index.
pub unsafe fn named_by_index(idx: u32) -> Option<(&'static [u8], u32)> {
    let mut n = 0;
    for i in 0..CHAN_MAX {
        if G_CHANNELS[i].used && G_CHANNELS[i].name_len > 0 {
            if n == idx {
                let len = G_CHANNELS[i].name_len as usize;
                return Some((
                    &G_CHANNELS[i].name[..len],
                    i as u32,
                ));
            }
            n += 1;
        }
    }
    None
}
