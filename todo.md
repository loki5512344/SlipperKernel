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
- [x] **procfs** — виртуальная ФС с информацией о системе:
  - /proc/version — версия ядра
  - /proc/cpuinfo — модель, частота, кол-во ядер
  - /proc/meminfo — всего ОЗУ, свободно, занято
  - /proc/uptime — время работы системы
  - /proc/load — нагрузка на CPU (процессы)
  - /proc/stat — статистика ядра
- [x] Интеграция с VFS — монтирование на /proc при старте (mount table / procfs module)

### Приоритет 3 — /font/ и шрифты:
- [x] **psfgen** — инструмент генерации PSF1 шрифта (256 glyphs, 8x16)
- [x] **PSF1/PSF2 парсер** — `kernel/src/font/mod.rs` с `font::init()`, `font::glyph_bitmap()`
- [x] **Загрузка шрифта** — ядро читает `/font/default.psf` после монтирования root
- [ ] **Поддержка Unicode таблицы** в PSF1/PSF2 (glyph → unicode mapping)

### Приоритет 4 — IPC:
- [ ] **IPC channels** — chan_create/connect/send/recv для root↔user коммуникации
- [ ] **`/ipc/*` виртуальный путь** в VFS

### Приоритет 5 — Драйверы:
- [ ] **FDT parser** — сейчас hardcoded QEMU адреса, нужен настоящий парсер device tree
- [ ] **PLIC IRQ dispatch** — сейчас просто log, нужна диспетчеризация прерываний
- [ ] **Framebuffer драйвер** — вывод на экран + шрифты
- [ ] **SDHCI драйвер** — для Milk-V Duo S

### Приоритет 6 — Инструменты:
- [ ] **elf2onx v2** — сейчас пишет v1, нужно использовать v2 формат с compressed_size
- [ ] **mkimage v2** — создание v2 образов с snapshot area + journal

### Приоритет 7 — Общее:
- [ ] **Panic recovery** — сейчас panic = halt, нужен kdump
- [ ] **Multi-core (SMP)** — сейчас 1 hart
