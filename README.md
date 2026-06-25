<p align="center">
  <img src="https://img.shields.io/badge/platform-RISC--V%2064--bit-green" alt="RISC-V 64">
  <img src="https://img.shields.io/badge/language-Rust%20%7E98%25-orange" alt="Rust ~98%">
  <img src="https://img.shields.io/badge/version-v0.3-blue" alt="v0.3">
  <img src="https://img.shields.io/badge/MMU-Sv39-yellow" alt="Sv39 MMU">
  <img src="https://img.shields.io/badge/license-GPL--3.0-red" alt="GPL-3.0">
  <a href="README.ru.md"><img src="https://img.shields.io/badge/ru_readme-blue" alt="ru_readme"></a>
</p>

<h1 align="center">OnyxKernel</h1>

<p align="center">
<pre class="not-prose" style="text-align:center;font-family:monospace;">
  ___                    _       ___  ____  
 / _ \ _ __   ___ _ __ / \     / _ \/ ___| 
| | | | '_ \ / _ \ '__/ _ \   | | | \___ \ 
| |_| | | | |  __/ | / ___ \  | |_| |___) |
 \___/|_| |_|\___|_|/_/   \_\  \___/|____/ 
</pre>
</p>

<p align="center"><em>A RISC-V 64-bit operating system kernel written in Rust</em></p>

----

OnyxKernel is a 64-bit RISC-V operating system kernel (~98% Rust, assembly via
`global_asm!`) featuring a three-ring privilege model, a custom filesystem with
journaling and snapshots, preemptive multitasking, and a complete userland with
authentication.

Part of the [OnyxOS](https://github.com/anomalyco/OnyxOS) ecosystem. Booted by
[OnyxBoot](https://github.com/anomalyco/OnyxBoot).

----

## Key Features

- **Written in Rust** — ~98% Rust, privileged assembly via `global_asm!` macros
- **RISC-V 64-bit (rv64gc)** — Sv39 MMU, three-level page tables
- **3-ring privilege model** — Kernel (ring 0), Root Space (ring 1), User Space (ring 2)
- **Dynamic processes** — heap-allocated linked list, no `PROC_MAX` limit
- **OnyxExec v2 binary format** — dynamic segments (up to 256), ring1 flag, compression flag
- **OnyxFS v2 filesystem** — timestamps (crtime/mtime/atime/ctime), indirect blocks, 40-byte dirents
- **Flashback snapshots** — `snapshot_create` / `rollback` / `list` with RLE compression + COW data blocks
- **Write-ahead journal** — crash recovery on mount
- **VFS with mount table** — OnyxFS, procfs, ipcfs
- **IPC channels** — `chan_create` / `connect` / `send` / `recv` / `close`; named channels via `/ipc/*` VFS
- **Syscall ABI** — 31 syscalls: `spawn`, `wait`, `read`, `write`, `exec`, `sbrk`, `kill`, `sigmask`, `snapshot_*`, `create`, `mkdir`, etc.
- **Preemptive multitasking** — timer tick scheduling with `NEED_RESCHED` → `sched_yield`
- **Signal delivery** — `SYS_kill`, `SIGKILL` terminates process
- **Blocking wait** — `Waiting` state + `sched_yield` for child process notification
- **Framebuffer console** — PSF1/PSF2 font support, `/font/default.psf` loaded at boot
- **Hardware drivers** — UART (NS16550A), VirtIO block, PCI, PLIC
- **FDT parser** — device tree discovery (memory, PLIC, devices)
- **Per-process FD table** — 16 slots per process with capability tokens
- **Userland** — `/bin/login`, `/bin/osh` (shell), `/bin/passwd`, `/bin/useradd`, `/bin/userdel`
- **Authentication** — `/etc/passwd` + `/etc/shadow`; first-boot interactive root password setup
- **/proc filesystem** — `version`, `cpuinfo`, `meminfo`, `uptime`, `load`, `stat`

----

## Architecture

### Privilege Rings

| Ring | Name | Mode | Description |
|------|------|------|-------------|
| 0 | Kernel | S-mode | Full access: memory, CSR, interrupts, drivers |
| 1 | Root Space | U-mode | PID 1 (`/bin/init`); all syscalls; can `spawn`, `create`, `snapshot` |
| 2 | User Space | U-mode | Restricted syscall ACL; cannot access `/service/*` |

- `dropring(target)` — one-way privilege drop: 0→1, 0→2, 1→2. Privilege escalation is prevented.
- `/bin/login` drops from ring 1 to ring 2, then `exec`s `/bin/osh`.

### Memory Layout (Sv39)

```
0x0000_0000_0000_0000 ──────────────────────
                         (unused)
0x0000_0000_0001_0000  user_base
                         ┌─────────────────────┐
                         │  code/data segments  │
                         ├─────────────────────┤
0x0000_0000_0100_0000  heap_base
                         │  user heap          │  (~252 MB via sbrk)
                         ├─────────────────────┤
0x0000_0000_2000_0000  ustack_top
                         │  user stack         │  (16 pages, 64 KB)
0x0000_0000_4000_0000 ──────────────────────
                         (kernel identity mapping, first 3 GB)
```

### Boot Chain

1. **OnyxBoot** (M-mode) — loads `kernel.elf` from VirtIO/SDHCI, jumps to entry
2. **boot.S** (`kernel/src/arch/asm/boot.rs`) — clears BSS, sets up PMP, delegates exceptions to S-mode, `mret` → `kmain`
3. **kmain** (`kernel/src/srv/main.rs`) — UART, FDT, PMM, VMM, heap, traps, PLIC, VirtIO, VFS mount, load `/bin/init`, `enter_user(1)`

### Process Lifecycle

```
alloc_proc() → Ready → Running → Exited
                  ↑         ↓
                  └── sched_yield()
                          
Running ──wait()──→ Waiting ──child exits──→ Ready
Running ──kill()──→ Exited (SIGKILL)
```

### OnyxExec v2 Binary Format

```
[0..3]   MAGIC "ONX\0"
[4..7]   flags (bit 0 = RING1, bit 1 = COMPRESSED)
[8..11]  entry offset
[12..15] num_segments
[16..23] entry_vaddr
──────── seg[0..num_segments] (32 bytes each) ────────
  [0..7]   vaddr
  [8..11]  filesz
  [12..15] memsz
  [16..19] offset (in file)
  [20..23] flags (VMM_R, VMM_W, VMM_X)
  [24..27] compressed_size (0 = uncompressed)
──────── raw segment data ────────
```

### Directory Structure

```
/bin/        — executables (init, login, osh, passwd, useradd, userdel, hello)
/service/    — root services (started by init)
/etc/        — configuration (passwd, shadow)
/proc/       — system information (version, cpuinfo, meminfo, uptime, load, stat)
/font/       — fonts (default.psf)
/users/      — user home directories (/users/username/)
```

----

## Building

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust nightly | ≥ 1.85 | Kernel & userland compilation |
| `riscv64-elf-gcc` | — | OnyxBoot cross-compilation |
| `qemu-system-riscv64` | — | Testing (optional) |
| `parted`, `mkfs.fat`, `mcopy` | — | Boot disk image creation |
| `make` | — | OnyxBoot build |

### Install Rust target

```console
$ rustup target add riscv64gc-unknown-none-elf
```

### Build the kernel

```console
$ cargo build --release --target riscv64gc-unknown-none-elf
```

### Full build + QEMU (one command)

```console
$ ./scripts/run_qemu.sh
```

This script:
1. Builds **OnyxBoot** bootloader (`make -C ../OnyxBoot`)
2. Builds **OnyxKernel**, **init**, and **tools** (`elf2onx`, `mkimage`, `psfgen`)
3. Converts userland ELFs → `.onx` via `elf2onx`
4. Generates PSF1 font via `psfgen`
5. Creates `disk.img` (OnyxFS) from manifest via `mkimage`
6. Creates partitioned `boot.img` (FAT32 + OnyxFS)
7. Launches QEMU

### Override OnyxBoot path

```console
$ ONYXBOOT_DIR=/path/to/OnyxBoot ./scripts/run_qemu.sh
```

----

## Running in QEMU

```console
$ qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios /path/to/OnyxBoot/bootloader.bin \
    -drive file=build/boot.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -no-reboot
```

Expected output:

```
OnyxBoot v0.4 [riscv-virtio,qemu]

OnyxBoot boot menu
--------------------
  0: VirtIO @ 0x0000000010008000
--------------------
Select device (0-0, or enter for auto): loading kernel.elf [########]
jumping to kernel
[kernel] OnyxKernel v0.3 — RISC-V 64-bit
[kernel] FDT parsed, memory: 256 MB
[kernel] PMM: bitmap + slab allocator ready
[kernel] VMM: Sv39 page tables initialized
[kernel] VirtIO block device probed
[kernel] OnyxFS mounted on /
[kernel] Loading /bin/init (ring 1)
[init] OnyxOS init v0.3 (root space)
[init] Starting /bin/login

OnyxOS Login
login: root
password: 
[login] dropping to ring 2
osh> _
```

----

## Project Structure

| Path | Description |
|------|-------------|
| `kernel/` | Kernel core: arch, proc, mm, fs, drivers, syscall, ipc |
| `core/` | Shared library: OnyxExec/OnyxFS format definitions, parser, errno |
| `init/` | Userland binaries: init, login, osh, passwd, useradd, userdel |
| `tools/` | Host tools: `elf2onx`, `mkimage`, `psfgen` |
| `scripts/` | Build & run scripts (`run_qemu.sh`) |
| `docs/` | Architecture documentation |

### Kernel Module Map

| Module | Path | Description |
|--------|------|-------------|
| `arch` | `kernel/src/arch/` | RISC-V boot, traps, CSR, SMP, register definitions |
| `proc` | `kernel/src/proc/` | Process management, scheduler, spawn, signals |
| `mm` | `kernel/src/mm/` | PMM (bitmap + slab), VMM (Sv39), kernel heap |
| `fs/onyxfs` | `kernel/src/fs/onyxfs/` | OnyxFS v2: read, write, mkdir, journal, snapshots |
| `fs/vfs` | `kernel/src/fs/vfs/` | Virtual filesystem: mount table, FD tokens, open/close/read/write |
| `fs/procfs` | `kernel/src/fs/procfs/` | /proc filesystem: version, cpuinfo, meminfo, uptime, load, stat |
| `fs/ipcfs` | `kernel/src/fs/ipcfs/` | /ipc/* VFS entries for named IPC channels |
| `drivers` | `kernel/src/drivers/` | UART, VirtIO block, PCI, PLIC, framebuffer, fb_term |
| `syscall` | `kernel/src/syscall/` | Syscall ABI, handler, ACL, dispatch |
| `ipc` | `kernel/src/ipc/` | IPC channels: create, connect, send, recv, close |
| `srv` | `kernel/src/srv/` | kmain, trap handler, timer, kernel log |
| `font` | `kernel/src/font/` | PSF1/PSF2 font parser and glyph renderer |
| `libfdt` | `kernel/src/libfdt/` | Flattened Device Tree parser |

----

## Related Projects

| Project | Description |
|---------|-------------|
| [OnyxBoot](https://github.com/anomalyco/OnyxBoot) | Minimalist RISC-V 64-bit bootloader (C++20, ~9.5 KB) |
| [OnyxCompiller](https://github.com/anomalyco/OnyxCompiller) | C → RV64 compiler (runs on OnyxOS as `/bin/onyxcc`) |

----

## Roadmap

- [ ] Unicode table support in PSF1/PSF2 fonts
- [ ] IPC named channels via `/ipc/*` VFS
- [ ] FDT-driven hardware discovery (replace hardcoded addresses)
- [ ] PLIC IRQ dispatch (currently log-only)
- [ ] Framebuffer driver improvements
- [ ] SDHCI driver (for Milk-V Duo S)
- [ ] `elf2onx` v2 output (compressed_size field)
- [ ] `mkimage` v2 (snapshot area + journal in image)
- [ ] Panic recovery / kdump
- [ ] Multi-core (SMP) support

----

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
