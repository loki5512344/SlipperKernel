#![allow(non_upper_case_globals)]

pub const SYS_write: u64 = 1;
pub const SYS_read: u64 = 2;
pub const SYS_exit: u64 = 3;
pub const SYS_yield: u64 = 4;
pub const SYS_getpid: u64 = 5;
pub const SYS_brk: u64 = 6;
pub const SYS_mmap: u64 = 7;
pub const SYS_open: u64 = 8;
pub const SYS_close: u64 = 9;
pub const SYS_lseek: u64 = 10;
pub const SYS_stat: u64 = 11;
pub const SYS_exec: u64 = 12;
pub const SYS_sbrk: u64 = 13;
// New syscalls for Root Space / User Space:
pub const SYS_spawn: u64 = 14; // spawn(path, ring_hint) -> child_pid
pub const SYS_wait: u64 = 15; // wait(status_out) -> exited_pid
pub const SYS_readdir: u64 = 16; // readdir(dir, name_out, len) -> 1/0/-err
pub const SYS_getring: u64 = 17; // getring() -> 0|1|2
pub const SYS_dropring: u64 = 18; // dropping(target) -> 0/-EPERM
pub const SYS_snapshot_create: u64 = 19; // snapshot_create(name) -> snap_id/-err
pub const SYS_snapshot_rollback: u64 = 20; // snapshot_rollback(id) -> 0/-err
pub const SYS_snapshot_list: u64 = 21; // snapshot_list(buf, len) -> count/-err
pub const SYS_kill: u64 = 22; // kill(pid, signal) -> 0/-err
pub const SYS_sigmask: u64 = 23; // sigmask(how, sig) -> 0/-err
pub const SYS_write_fd: u64 = 24; // write_fd(fd, buf, len) -> written
pub const SYS_create: u64 = 25; // create(path, mode) -> fd
pub const SYS_mkdir: u64 = 26; // mkdir(path) -> 0/-err
pub const SYS_chan_create: u64 = 27; // chan_create() -> chan_id / -err  (root-only)
pub const SYS_chan_connect: u64 = 28; // chan_connect(chan_id) -> 0/-err
pub const SYS_chan_send: u64 = 29; // chan_send(chan_id, buf, len) -> n/-err
pub const SYS_chan_recv: u64 = 30; // chan_recv(chan_id, buf, len) -> n/-err
pub const SYS_chan_close: u64 = 31; // chan_close(chan_id) -> 0/-err

pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

// Ring constants for syscall arg.
pub const RING_KERNEL: u64 = 0;
pub const RING_ROOT: u64 = 1;
pub const RING_USER: u64 = 2;
