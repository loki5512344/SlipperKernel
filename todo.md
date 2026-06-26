# OnyxKernel — TODO

## ✅ Готово:
1. **Полный рерайт на Rust** (~98%, assembly через global_asm!)
2. **Динамические процессы** — нет PROC_MAX, heap-allocated linked list
3. **OnyxExec v2** — dynamic segments (до 256), ring1 flag, compression flag
4. **OnyxFS v2** — timestamps (crtime/mtime/atime/ctime), indirect blocks, dirents 40 bytes
5. **Flashback snapshots** — snapshot_create / rollback / list с RLE сжатием + COW data blocks
6. **Root/User Space** — 3 ring'а, syscall ACL, path-policy, dropring
7. **Syscalls (49)** — полная таблица ядерных вызовов:
   - **1-5**: write, read, exit, yield, getpid
   - **6-7**: brk, mmap ✅ (раньше были stubbed)
   - **8-13**: open, close, lseek, stat, exec, sbrk
   - **14-18**: spawn, wait, readdir, getring, dropring
   - **19-23**: snapshot_create/rollback/list, kill, sigmask
   - **24-26**: write_fd, create, mkdir
   - **27-33**: chan_create/connect/send/recv/close/create_named/open
   - **34-36**: munmap, dup, pipe (NEW)
   - **37-40**: unlink, rename, chdir, getcwd (NEW)
   - **41-44**: truncate, access, gettimeofday, fcntl (NEW)
   - **45-48**: getuid, getgid, utimens, uname (NEW)
   - **49**: nanosleep (NEW)
   - 🐛 **Fix**: SYS_chan_open(33) был пропущен в ACL — теперь доступен user-пространству
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

### Приоритет 1 — Userland:
- [x] **`/bin/login`** — аутентификация (root + пользователи из /etc/passwd), dropring(USER), exec(/bin/osh)
- [x] **`/bin/osh`** — пользовательский shell (ring 2) с командами ls/cat/echo/exec/clear/exit
- [x] **`/bin/passwd`** — смена пароля (root + self)
- [x] **`/bin/useradd`** — добавление пользователя (root only)
- [x] **`/bin/userdel`** — удаление пользователя (root only)
- [x] **`/etc/passwd`** + `/etc/shadow` — парсинг, аутентификация
- [x] **`/users/`** — домашние директории пользователей (/users/username/)
- [x] **Per-process FD table** — уже сделан (per-process VfsFd в Proc)
- [x] **add_dirent overwrite** — create теперь перезаписывает существующий dirent (вместо дублирования)
- [x] **First-boot setup** — нет дефолтных паролей; login запрашивает пароль root при первом запуске
- [x] **mkimage --add/--add-dir** — рекурсивное добавление директорий и отдельных файлов

### Приоритет 2 — /proc/ файловая система:
- [x] **procfs** — виртуальная ФС с информацией о системе

### Приоритет 3 — /font/ и шрифты:
- [x] **psfgen** + **PSF1/PSF2 парсер** + загрузка `/font/default.psf`
- [x] **Поддержка Unicode таблицы** — `glyph_for_cp()`, `glyph_or_default()`, psfgen mode=0x02

### Приоритет 4 — IPC:
- [x] **IPC channels** — chan_create/connect/send/recv для root↔user коммуникации
- [x] **`/ipc/*` виртуальный путь** в VFS через ipcfs (mount, lookup, readdir)

### Приоритет 5 — Драйверы:
- [ ] **SDHCI драйвер** — для Milk-V Duo S

### Приоритет 6 — Инструменты:
- [ ] **elf2onx v2** — v2 формат с compressed_size
- [ ] **mkimage v2** — v2 образы с snapshot area + journal

### Приоритет 7 — Общее:
- [x] **Panic recovery** — kdump (CSR, backtrace, hartid, dump_all), QEMU reboot
- [x] **Multi-core (SMP)** — G_HART_CURRENT, G_HART_IDLE_TF, SpinLock, sched_enter_idle()
- [ ] **SMP scheduler improvements** — per-CPU run queues, load balancing, CPU affinity
