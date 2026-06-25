# OnyxKernel — TODO

## ✅ Готово:
1. **Полный рерайт на Rust** (~98%, assembly через global_asm!)
2. **Динамические процессы** — нет PROC_MAX, heap-allocated linked list
3. **OnyxExec v2** — dynamic segments (до 256), ring1 flag, compression flag
4. **OnyxFS v2** — timestamps (crtime/mtime/atime/ctime), indirect blocks, dirents 40 bytes
5. **Flashback snapshots** — snapshot_create / rollback / list с RLE сжатием + COW data blocks
6. **Root/User Space** — 3 ring'а, syscall ACL, path-policy, dropring
7. **Syscalls** — spawn, wait, readdir, getring, dropping, kill, sigmask, snapshot_*, write_fd, create, mkdir
8. **OnyxFS write** — onyxfs_write(), create(), mkdir() с bitmap allocation
9. **Journal recovery** — write-ahead journal + recovery при mount
10. **I/O batching** — read_multi/write_multi для multi-sector I/O
11. **Preemption** — timer tick → sched_tick → NEED_RESCHED → sched_yield
12. **Блокирующий wait** — Waiting state + sched_yield
13. **Signal delivery** — SYS_kill, SIGKILL terminates
14. **Рефакторинг** — все файлы ≤150 строк
15. **QEMU verified** — ядро грузится, init работает в ring 1
16. **onx::load BSS page-fault fix** — `PTE_A | PTE_D` теперь выставляются для всех
    user-leaf PTE в сегментах / стеке / куче (раньше `map_one_pub` вызывался
    без A/D, что под QEMU с `menvcfg.ADUE = 0` приводило к page fault на
    первом обращении — типичный симптом: `onyxcc` падал на доступе к BSS
    по адресу `0x199f0`, где располагается первый глобал 1.2 MB сегмента).
17. **Unicode таблица в PSF1/PSF2** — glyph → unicode mapping, glyph_for_unicode(),
    glyph_bitmap_unicode(), UTF-8 декодирование и рендеринг в framebuffer
18. **IPC channels** — ipc::channel с create/create_named/open_by_name/connect/send/recv/close,
    блокирующий wait, ring buffer 4KB, up to 32 channels
19. **`/ipc/*` VFS** — ipcfs модуль: lookup/stat/read/write/readdir, mounted at /ipc
20. **FDT parser** — libfdt::fdt с полным DTB walk, find_memory/find_plic/find_clint/find_uart/find_virtio/model
21. **PLIC IRQ dispatch** — register_handler/dispatch, up to 64 IRQ handlers
22. **Framebuffer драйвер** — 32bpp, PSF1/PSF2, draw_char/draw_str/scroll/fb_term
23. **SMP (multi-core)** — secondary hart boot, per-hart current proc, scheduler spinlock,
    secondary harts enter idle→scheduler loop
24. **Panic recovery (kdump)** — stack trace (frame pointer walk), process list dump,
    QEMU reboot via test finisher

## ❌ Осталось сделать:

### Приоритет 5 — Драйверы:
- [ ] **SDHCI драйвер** — для Milk-V Duo S (CMD0→CMD8→ACMD41→CMD2→CMD3→CMD9→CMD7→CMD16)

### Приоритет 6 — Инструменты:
- [ ] **elf2onx v2** — сейчас пишет v1, нужно использовать v2 формат с compressed_size
- [ ] **mkimage v2** — создание v2 образов с snapshot area + journal

### Приоритет 7 — Общее:
- [ ] **SMP scheduler improvements** — per-CPU run queues, load balancing, CPU affinity
- [ ] **Panic recovery improvements** — register dump from trap frame, kernel core dump to disk
