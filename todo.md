## OnyxBoot

**Нет, исходный код OnyxBoot я не менял.** Единственное что сделал — поправил `Makefile` (добавил `-fno-pie -fno-pic -static -no-pie` флаги и исправил табы), чтобы он собирался `riscv64-linux-gnu-g++` вместо `riscv64-elf-g++`. Сам bootloader (C++ код) остался как есть.

---

## Что сделано vs что осталось

### ✅ Готово:
1. **Полный рерайт на Rust** (~98%)
2. **Динамические процессы** — нет PROC_MAX, heap-allocated linked list
3. **OnyxExec v2** — dynamic segments, ring1 flag, compression flag
4. **OnyxFS v2** — timestamps (crtime/mtime/atime/ctime), indirect blocks, dirents 40 bytes
5. **Flashback snapshots** — `snapshot_create` / `rollback` / `list` (stub — копирует inode table + bitmap)
6. **Root/User Space** — 3 ring'а, syscall ACL, path-policy, dropring
7. **Syscalls** — spawn, wait, readdir, getring, dropping + 3 snapshot syscall'а
8. **QEMU verified** — ядро грузится, init работает в ring 1

### ❌ Осталось сделать:

**OnyxFS:**
- [ ] **Реальная запись** — `onyxfs_write()`, `onyxfs_create()`, `onyxfs_mkdir()` (сейчас ФС read-only)
- [ ] **Snapshot data COW** — сейчас копируется только inode table, нужно Copy-on-Write для data blocks
- [ ] **Сжатие snapshots** — LZ4/RLE для сжатия snapshot данных (infrastructure есть, кода нет)
- [ ] **I/O batching** — multi-sector virtio reads (сейчас 8 секторов по одному)
- [ ] **Journal recovery** — запись в journal перед изменениями + recovery при boot

**Процессы:**
- [ ] **Блокирующий wait** — сейчас `SYS_wait` возвращает `ENOENT` вместо блокировки (нет preemption)
- [ ] **Preemption** — timer tick должен реально переключать процессы (сейчас cooperative)
- [ ] **Signal delivery** — `SYS_kill` для отправки сигналов

**Root/User Space:**
- [ ] **`/bin/login`** — аутентификация + `dropring(USER)` + `exec(/bin/osh)`
- [ ] **`/bin/osh`** — пользовательский shell (ring 2)
- [ ] **`/etc/passwd`** + `/etc/shadow` — парсинг, аутентификация
- [ ] **IPC channels** — `chan_create/connect/send/recv` для root↔user коммуникации
- [ ] **Per-process FD table** — сейчас FD глобальные, нужны per-process

**Драйверы:**
- [ ] **FDT parser** — сейчас hardcoded QEMU адреса, нужен настоящий парсер device tree
- [ ] **PLIC IRQ dispatch** — сейчас просто log, нужна диспетчеризация прерываний
- [ ] **Real hardware** — SDHCI драйвер для Milk-V Duo S

**Инструменты:**
- [ ] **mkimage с поддиректориями** — `--add-dir`, `--add file path img` для создания полной структуры `/bin/`, `/service/`, `/etc/`
- [ ] **elf2onx v2** — сейчас пишет v1, нужно использовать v2 формат с compressed_size

**Общее:**
- [ ] **Panic recovery** — сейчас panic = halt, нужен kdump
- [ ] **Multi-core (SMP)** — сейчас 1 hart
