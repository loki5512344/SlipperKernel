use crate::arch::csr;
use crate::arch::regs::*;
use onyx_core::fmt::{vformat, Arg, Write};

const MAX_BT_DEPTH: usize = 64;

unsafe fn backtrace(w: &mut impl Write) {
    let mut fp: u64;
    core::arch::asm!("mv {}, s0", out(reg) fp);
    for i in 0..MAX_BT_DEPTH {
        if fp == 0 || fp & 0xf != 0 {
            break;
        }
        let ra = *(fp as *const u64).sub(1);
        let old_fp = *(fp as *const u64).sub(2);
        let args: &[Arg] = &[Arg::from(i), Arg::from(ra)];
        vformat(w, "  [%d] ra=%p\n", args);
        if ra == 0 {
            break;
        }
        fp = old_fp;
    }
}

pub unsafe fn kdump() {
    let mut w = crate::srv::klog::PanicWriter;
    w.write_str("\n--- KDUMP ---\n");

    let hartid: u64;
    core::arch::asm!("mv {}, tp", out(reg) hartid);
    let args: &[Arg] = &[Arg::from(hartid)];
    vformat(&mut w, "hartid=%d\n", args);

    let sepc = csr::read_sepc();
    let sstatus = csr::read_sstatus();
    let scause = csr::read_scause();
    let stval = csr::read_stval();
    let satp = csr::read_satp();
    let sie = csr::read_sie();
    let args: &[Arg] = &[
        Arg::from(sepc),
        Arg::from(sstatus),
        Arg::from(scause),
        Arg::from(stval),
        Arg::from(satp),
        Arg::from(sie),
    ];
    vformat(
        &mut w,
        "sepc=%p sstatus=%p scause=%p stval=%p satp=%p sie=%p\n",
        args,
    );

    let pid = crate::proc::current_pid();
    if pid != 0 {
        let args: &[Arg] = &[Arg::from(pid)];
        vformat(&mut w, "pid=%d\n", args);
    }
    if let Some(p) = crate::proc::current_opt() {
        let args: &[Arg] = &[Arg::from(p.ring), Arg::from(p.parent_pid)];
        vformat(&mut w, "ring=%d parent=%d\n", args);
    }

    let cnt = crate::proc::count();
    let args: &[Arg] = &[Arg::from(cnt)];
    vformat(&mut w, "processes=%d\n", args);

    let online = crate::arch::smp::online_harts();
    let args: &[Arg] = &[Arg::from(online)];
    vformat(&mut w, "online_harts=%d\n", args);

    w.write_str("Backtrace:\n");
    backtrace(&mut w);

    w.write_str("--- END KDUMP ---\n");
}
