# OnyxKernel — Архитектура

## 1. Обзор

OnyxKernel — RISC-V 64 (rv64gc) ядро с трёхуровневой изоляцией:

- **Ring 0 (Kernel / S-mode)** — ядро, драйверы, VFS, VMM, планировщик
- **Ring 1 (Root Space)** — привилегированное пользовательское пространство (init, сервисы)
- **Ring 2 (User Space)** — непривилегированное пользовательское пространство (shell, юзерские программы)

Бинарник init (`/bin/init`) запускается как PID 1 в Ring 1. Он сканирует `/service/` на предмет `*.bin`/`*.osh` файлов, запускает каждый как root-сервис, затем запускает `/bin/login`. Login аутентифицирует пользователя и через `dropring(2)` опускается в Ring 2, после чего exec-ит `/bin/osh` — пользовательский shell.

---

## 2. Цепочка загрузки

### 2.1 boot.S (`kernel/src/arch/asm/boot.rs`)
- Входная точка `_start` в M-mode
- Сохраняет hartid и FDT адрес
- Обнуляет BSS
- Настраивает PMP (Physical Memory Protection) — разрешает U-mode доступ ко всей памяти
- Устанавливает `medeleg`/`mideleg` — делегирует исключения и прерывания в S-mode
- Переключается в S-mode через `mret` в `kmain`

### 2.2 kmain (`kernel/src/srv/main.rs`)
- Инициализация UART (последовательный порт)
- Парсинг FDT (Flattened Device Tree) — определение памяти, PLIC, устройств
- Инициализация PMM (Physical Memory Manager) — bitmap + slab аллокатор
- Инициализация VMM (Virtual Memory Manager) — Sv39 страничная адресация
- Инициализация кучи (kernel heap allocator)
- Инициализация trap-обработчика и таймера
- Инициализация PLIC (Platform-Level Interrupt Controller)
- Probe/init VirtIO блочных устройств
- VFS: mount корневой файловой системы (OnyxFS)
- Чтение `/bin/init` с диска, загрузка через `onx::load()`
- Создание процесса PID 1 в Ring 1 (если бинарник имеет флаг RING1)
- `enter_user(1)` — переход в user-space

---

## 3. Ring-модель и изоляция

| Ring | Название | U/S | PID 1? | Доступ |
|------|----------|-----|--------|--------|
| 0 | Kernel | S-mode | нет | полный доступ ко всей памяти, CSR, прерываниям |
| 1 | Root Space | U-mode | да | все syscalls, включая spawn, wait, snapshot, create, mkdir, kill |
| 2 | User Space | U-mode | нет | ограниченный набор syscalls; нет доступа к /service/* |

### 3.1 Ограничения Ring 2 (User)
- **Syscall ACL** (`kernel/src/syscall/handler.rs:38-59`): spawn, wait, kill, create, mkdir, snapshot, chan_create запрещены
- **Path policy** (`kernel/src/syscall/fs_sys.rs:96-99`): User-процессы не могут открыть файлы из `/service/`

### 3.2 Dropring
- `SYS_dropping(target)` — однонаправленный переход: 0→1, 0→2, 1→2
- Нельзя повысить привилегии (проверка `target < p.ring`)

---

## 4. Инициализация сервисов (init)

**PID 1** — `/bin/init` (`init/src/main.rs`):
1. Пишет баннер `[init] OnyxOS init v0.3 (root space)`
2. Сканирует `/service/` через `SYS_readdir`
3. Для каждого `*.bin` / `*.osh` — вызывает `SYS_spawn(path, 1)` (Ring 1)
4. Запускает `/bin/login` через `SYS_spawn`
5. Входит в reaper loop: `SYS_wait()` + `SYS_yield()`

---

## 5. Аутентификация и вход (login)

**`/bin/login`** (`init/src/login.rs`):
1. Выводит `OnyxOS Login`
2. Читает username и password через `SYS_read`
3. MVP-логика: любой непустой username/password → успех
4. `SYS_dropping(2)` — опускает привилегии в Ring 2
5. `SYS_exec("/bin/osh")` — запускает пользовательский shell

---

## 6. Пользовательский Shell (osh)

**`/bin/osh`** (`init/src/osh.rs`):
- Работает в Ring 2 (user space)
- Команды: `help`, `echo`, `cat`, `ls`, `exec`, `clear`, `exit`, `whoami`, `pwd`
- `whoami` вызывает `SYS_getring()` и выводит "root" (ring 1) или "user" (ring 2)
- `cat`/`ls` используют `SYS_open`/`SYS_read`/`SYS_readdir`
- `exec` использует `SYS_exec` — заменяет текущий процесс на новый бинарник
- `exit` вызывает `SYS_exit(0)`

---

## 7. Системные вызовы

### 7.1 Диспетчеризация
- Trap handler (`kernel/src/srv/trap.rs`) ловит `ecall` (CAUSE_U_ECALL)
- Вызывает `handler::handle()` (`kernel/src/syscall/handler.rs`)
- Всего 31 syscall (см. `kernel/src/syscall/abi.rs`)

### 7.2 ACL
```rust
fn syscall_allowed(nr: u64, ring: u8) -> bool {
    match nr {
        SYS_write | SYS_read | SYS_exit | SYS_yield | SYS_getpid
        | SYS_sbrk | SYS_open | SYS_close | SYS_lseek | SYS_stat
        | SYS_exec | SYS_readdir | SYS_getring | SYS_dropring
        | SYS_sigmask | SYS_write_fd | SYS_chan_connect
        | SYS_chan_send | SYS_chan_recv | SYS_chan_close => true,
        SYS_spawn | SYS_wait | SYS_snapshot_create
        | SYS_snapshot_rollback | SYS_snapshot_list
        | SYS_kill | SYS_create | SYS_mkdir | SYS_chan_create
        => ring <= PROC_RING_ROOT,
        _ => false,
    }
}
```

---

## 8. Процессы

### 8.1 Структура (`kernel/src/proc/process.rs`)
```rust
pub struct Proc {
    pid: u32,
    ring: u8,            // 0, 1, или 2
    state: ProcState,    // Free, Ready, Running, Exited, Waiting
    parent_pid: u32,
    exit_code: i32,
    root_pa: u64,        // корень страничных таблиц (Sv39)
    entry: u64,
    ustack: u64,
    heap_brk: u64,
    uid: u32,
    gid: u32,
    tf: TrapFrame,
    kstack: [u8; 16K],
    pending_signals: u32,
    signal_mask: u32,
    fds: [VfsFd; 16],    // per-process FD table
    next: *mut Proc,      // linked list
}
```

### 8.2 Жизненный цикл
- `alloc_proc()` — создание узла, добавление в глобальный связный список
- `free_proc()` — удаление из списка, освобождение памяти
- `enter_user()` — установка `G_CURRENT`, переход в U-mode через `drop_to_user`
- `exit()` — установка `Exited`, будим родителя если он в `Waiting`
- `wait()` — блокировка до выхода дочернего процесса

### 8.3 Создание процесса
- `spawn()` (`kernel/src/proc/spawn.rs:52`) — открывает `.onx` файл через VFS, читает, загружает сегменты через `onx::load()`, выделяет PID, вызывает `create_user()`
- `exec()` (`kernel/src/syscall/fs_sys2.rs:14`) — заменяет текущий процесс (очищает старые страничные таблицы, загружает новые)

---

## 9. Память

### 9.1 Физическая память (`kernel/src/mm/pmm/`)
- `bitmap.rs` — bitmap аллокатор для фреймов 4K
- `slab.rs` — slab аллокатор для маленьких объектов

### 9.2 Виртуальная память (`kernel/src/mm/vmm/`)
- Sv39, 3 уровня страничных таблиц
- `new_root()` / `destroy_root()` — создание/удаление адресного пространства
- `map_one_pub()` — отображение одной страницы с U-флагом
- Первые 3 GB (0x0-0xC0000000) отображены 1:1 (kernel identity mapping)

### 9.3 User address space
```
0x0000_0000_0000_0000 ──────────────────────
                         (не используется)
0x0000_0000_0001_0000  user_base
                         ┌─────────────────┐
                         │ сегменты кода/данных │
                         ├─────────────────┤
0x0000_0000_0100_0000  heap_base
                         │  user heap      │  (~252 MB)
                         ├─────────────────┤
0x0000_0000_2000_0000  ustack_top
                         │  user stack     │  (16 страниц)
0x0000_0000_4000_0000 ──────────────────────
                         (kernel mapping 1:1)
```

---

## 10. Файловая система

### 10.1 OnyxFS (`kernel/src/fs/onyxfs/`)
- Собственная ФС с журналом (write-ahead journal), снэпшотами (Flashback), сжатием RLE
- Поддерживает: создание, удаление, запись, чтение директорий
- inode-based: crtime, mtime, atime, ctime, indirect blocks
- Размер блока: 512 байт (один сектор virtio-blk)

### 10.2 VFS (`kernel/src/fs/vfs/`)
- Прослойка между syscalls и конкретной ФС
- Capability FD tokens (64-bit: 32 бита индекс + 32 бита epoch)
- Per-process FD таблицы (16 слотов на процесс)
- Операции: open, close, read, write, lseek, stat, readdir, create, mkdir

---

## 11. Межпроцессное взаимодействие (IPC)

### 11.1 Каналы (`kernel/src/ipc/`)
- `chan_create()` — создание канала (root-only)
- `chan_connect()` — подключение к каналу (любой процесс)
- `chan_send()` / `chan_recv()` — передача данных
- `chan_close()` — закрытие канала

---

## 12. Он-лайф формат (OnyxExec / .onx)

`tools/src/elf2onx.rs` конвертирует ELF → .onx.

### 12.1 Формат (v2)
```
[0..3]   MAGIC "ONX\0"
[4..7]   flags (bit 0 = RING1, bit 1 = COMPRESSED)
[8..11]  entry offset (в финальном сегменте)
[12..15] num_segments
[16..23] entry_vaddr
──────── seg[0..num_segments] ────────
  [0..7]  vaddr
  [8..11] filesz
  [12..15] memsz
  [16..19] offset (в файле)
  [20..23] flags (VMM_R, VMM_W, VMM_X)
  [24..27] compressed_size (0 = несжатый)
  ─── padding до 32 байт ───
──────── raw segment data ────────
```

Флаг `ONX_FLAGS_RING1` определяет, что бинарник запускается в Ring 1 (root space). Без флага — Ring 2 (user space).

---

## 13. Снэпшоты (Flashback)

- `SYS_snapshot_create(name)` — создаёт снепшот текущего состояния ФС
- `SYS_snapshot_rollback(id)` — откатывает ФС к состоянию снепшота
- `SYS_snapshot_list(buf, len)` — список всех снепшотов
- Использует RLE-сжатие + COW (copy-on-write) для блоков данных
- Журнал (journal) гарантирует целостность при сбоях

---

## 14. Структура директорий

```
/bin/             — бинарники (init, login, osh)
/service/         — root-сервисы (запускаются init-ом)
/dev/             — устройства
/etc/             — конфиги
```

---

## 15. Сборка и запуск

```bash
# Сборка
cargo build --release --target riscv64gc-unknown-none-elf
# Запуск в QEMU
./scripts/run_qemu.sh
```

**TODO** (`todo.md`): `/etc/passwd` + `/etc/shadow` для настоящей аутентификации, полноценный IPC, SMP, драйверы для реального железа.
