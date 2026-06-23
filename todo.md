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
- [ ] **`/bin/login`** — аутентификация (root + пользователи из /etc/passwd), dropring(USER), exec(/bin/osh)
- [ ] **`/bin/osh`** — пользовательский shell (ring 2) с командами ls/cat/echo/exec/clear/exit
- [ ] **`/bin/passwd`** — смена пароля (root + self)
- [ ] **`/bin/useradd`** — добавление пользователя (root only)
- [ ] **`/bin/userdel`** — удаление пользователя (root only)
- [ ] **`/etc/passwd`** + `/etc/shadow` — парсинг, аутентификация
- [ ] **`/users/`** — домашние директории пользователей (/users/username/)
- [ ] **Per-process FD table** — сейчас FD глобальные, нужны per-process
- [ ] **mkimage с поддиректориями** — --add-dir, --add file path для /bin/ /service/ /etc/ /users/ /font/

### Приоритет 2 — /proc/ файловая система:
- [ ] **procfs** — виртуальная ФС с информацией о системе:
  - /proc/version — версия ядра
  - /proc/cpuinfo — модель, частота, кол-во ядер
  - /proc/meminfo — всего ОЗУ, свободно, занято
  - /proc/uptime — время работы системы
  - /proc/load — нагрузка на CPU (общая + по ядрам)
  - /proc/stat — статистика ядра
- [ ] Интеграция с VFS — монтирование на /proc при старте

### Приоритет 3 — /font/ и шрифты:
- [ ] **Поддержка шрифтов в VGA/framebuffer**:
  - /font/default.psf — основной шрифт (PSF формат)
  - Загрузка шрифта при инициализации терминала
  - Поддержка Unicode таблицы в шрифте

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
