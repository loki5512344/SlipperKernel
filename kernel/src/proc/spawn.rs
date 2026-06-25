//! Process spawning and waiting — `create_user`, `spawn`, `wait`.
//!
//! `spawn` is the SYS_spawn implementation: it loads a .onx file via the VFS,
//! constructs a fresh process via `create_user`, and returns its PID. `wait`
//! is SYS_wait: it blocks the caller until a child exits, then reaps it.
use super::lifecycle::{alloc_proc, free_proc};
use super::process::{
    alloc_pid, by_pid, current_for_hart, current_pid, hart_id, ProcState, G_PROC_LIST,
    PROC_RING_ROOT, PROC_RING_USER,
};
use crate::arch::regs::*;
use crate::arch::trap_frame::TrapFrame;
use crate::mm::heap;
use crate::proc::onx;
use onyx_core::errno::{Errno, KResult};

/// Create a user-mode process (ring 1 or 2). Dynamic allocation — no limit.
pub unsafe fn create_user(
    entry: u64,
    ustack: u64,
    root_pa: u64,
    pid: u32,
    parent_pid: u32,
    heap_brk: u64,
    ring: u8,
    argc: usize, 
    argv_sp: u64,
) -> KResult<()> {
    if entry == 0 {
        crate::kerr!("create_user", "entry=0 — would cause page fault, rejecting");
        return Err(Errno::Inval);
    }
    let p = alloc_proc()?;
    (*p).pid = pid;
    (*p).ring = ring;
    (*p).state = ProcState::Ready;
    (*p).parent_pid = parent_pid;
    (*p).exit_code = 0;
    (*p).root_pa = root_pa;
    (*p).entry = entry;
    (*p).ustack = ustack;
    (*p).heap_brk = heap_brk;
    (*p).uid = if ring <= PROC_RING_ROOT { 0 } else { 1000 };
    (*p).gid = (*p).uid;
    (*p).tf = TrapFrame::zero();
    (*p).pending_signals = 0;
    (*p).signal_mask = 0;
    (*p).tf.sepc = entry;
    (*p).tf.sp = if argc > 0 { argv_sp } else { ustack };
    (*p).tf.a0 = argc as u64;
    (*p).tf.a1 = if argc > 0 { argv_sp + 8 } else { 0 };
    (*p).tf.sstatus = SSTATUS_SPIE;
    (*p).tf.satp = SATP_MODE_SV39 | (root_pa >> 12);
    Ok(())
}

/// **SYS_spawn**: create a new process from .onx file without replacing current.
pub unsafe fn spawn(path: &[u8], argv_user: u64, ring_hint: u8, parent_pid: u32) -> KResult<u32> {
    use crate::fs::vfs;
    let token = vfs::open(path, vfs::PERM_READ | vfs::PERM_SEEK)?;
    let mut size = 0u32;
    vfs::stat(token, &mut size)?;
    if size == 0 {
        vfs::close(token)?;
        return Err(Errno::Inval);
    }
    let img = heap::kmalloc(size as usize)?;
    vfs::read(token, img, size)?;
    vfs::close(token)?;
    let r = onx::load(img, size as usize)?;
    heap::kfree(img);
    let new_pid = alloc_pid();
    let ring = if ring_hint == PROC_RING_ROOT && r.ring == 1 {
        PROC_RING_ROOT
    } else {
        PROC_RING_USER
    };
    let (argc, argv_sp) = if argv_user != 0 {
        crate::proc::onx::copy_argv_to_stack(r.root_pa, r.ustack, argv_user)
    } else {
        (0, 0)
    };
    create_user(
        r.entry, r.ustack, r.root_pa, new_pid, parent_pid, r.heap_brk, ring, argc, argv_sp,
    )?;
    Ok(new_pid)
}

/// **SYS_wait**: wait for any child to exit. Returns (pid, exit_code).
/// Blocks (sets current state to `Waiting` and yields) if no child has exited
/// yet but at least one child exists. The process is woken when a child calls
/// `exit()` (which transitions the parent back to `Ready`). Returns ENOENT if
/// the caller has no children at all.
///
/// Note on the control-flow quirk: `sched_yield` does not return to its caller
/// when it actually switches — it `sret`s to user space using the saved trap
/// frame. Because `tf.sepc` was not yet advanced past the `ecall` instruction
/// (we are still inside `handle()`), the user process re-executes the `ecall`,
/// re-entering `wait()` from the top. The loop below therefore executes at
/// most one iteration per `ecall`; the "retry" happens via re-ecall.
pub unsafe fn wait(tf: &mut TrapFrame, status_out: *mut i32) -> KResult<u32> {
    let my_pid = current_pid();
    // Look for exited child.
    let mut cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && matches!((*cur).state, ProcState::Exited) {
            let exited_pid = (*cur).pid;
            let code = (*cur).exit_code;
            if !status_out.is_null() {
                *status_out = code;
            }
            free_proc(cur);
            return Ok(exited_pid);
        }
        cur = (*cur).next;
    }
    // Check if any child exists.
    let mut has_child = false;
    cur = G_PROC_LIST;
    while !cur.is_null() {
        if (*cur).parent_pid == my_pid && !matches!((*cur).state, ProcState::Free) {
            has_child = true;
            break;
        }
        cur = (*cur).next;
    }
    if !has_child {
        return Err(Errno::NoEnt);
    }
    // Block: set state to Waiting and yield. `sched_yield` either switches to
    // another Ready process (and `sret`s away — control does not return here)
    // or, if no other process can run, halts the kernel (deadlock detected).
    // The `Err` below is unreachable in practice but keeps the type system
    // happy.
    let hartid = hart_id();
    let cur = current_for_hart(hartid);
    if !cur.is_null() {
        (*cur).state = ProcState::Waiting;
    }
    super::scheduler::sched_yield(tf);
    Err(Errno::NoEnt)
}
