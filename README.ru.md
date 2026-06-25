<p align="center">
  <img src="https://img.shields.io/badge/platform-RISC--V%2064--bit-green" alt="RISC-V 64">
  <img src="https://img.shields.io/badge/language-Rust%20%7E98%25-orange" alt="Rust ~98%">
  <img src="https://img.shields.io/badge/version-v0.3-blue" alt="v0.3">
  <img src="https://img.shields.io/badge/MMU-Sv39-yellow" alt="Sv39 MMU">
  <img src="https://img.shields.io/badge/license-GPL--3.0-red" alt="GPL-3.0">
  <a href="README.md"><img src="https://img.shields.io/badge/en_readme-blue" alt="en_readme"></a>
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

<p align="center"><em>Ядро операционной системы RISC-V 64-bit, написанное на Rust</em></p>

----

OnyxKernel — 64-битное RISC-V ядро ОС (~98% Rust, ассемблер через
`global_asm!`) с трёхуровневой моделью привилегий, собственной файловой системой
с журналированием и снэпшотами, вытесняющей многозадачностью и полноценным
юзерлендом с аутентификацией.

Часть экосистемы [OnyxOS](https://github.com/anomalyco/OnyxOS). Загружается
[OnyxBoot](https://github.com/anomalyco/OnyxBoot).

----

## Ключевые особенности

- **Написано на Rust** — ~98% Rust, привилегированный ассемблер через макросы `global_asm!`
- **RISC-V 64-bit (rv64gc)** — MMU Sv39, трёхуровневые страничные таблицы
- **3 кольца привилегий** — Ядро (ring 0), Root Space (ring 1), User Space (ring 2)
- **Динамические процессы** — heap-allocated связный список, нет лимита `PROC_MAX`
- **Бинарный формат OnyxExec v2** — динамические сегменты (до 256), флаг ring1, флаг сжатия
- **Файловая система OnyxFS v2** — временные метки (crtime/mtime/atime/ctime), косвенные блоки, 40-байтовые dirents
- **Снэпшоты Flashback** — `snapshot_create` / `rollback` / `list` с RLE-сжатием + COW блоки данных
- **Журнал предзаписи (WAL)** — восстановление после сбоев при монтировании
- **VFS с таблицей монтирования** — OnyxFS, procfs, ipcfs
- **IPC каналы** — `chan_create` / `connect` / `send` / `recv` / `close`; именованные каналы через `/ipc/*` VFS
- **Syscall ABI** — 31 системный вызов: `spawn`, `wait`, `read`, `write`, `exec`, `sbrk`, `kill`, `sigmask`, `snapshot_*`, `create`, `mkdir` и др.
- **Вытесняющая многозадачность** — планирование по таймеру, `NEED_RESCHED` → `sched_yield`
- **Доставка сигналов** — `SYS_kill`, `SIGKILL` завершает процесс
- **Блокирующий wait** — состояние `Waiting` + `sched_yield` для уведомления о завершении потомка
- **Framebuffer-консоль** — поддержка шрифтов PSF1/PSF2, загрузка `/font/default.psf` при старте
- **Драйверы оборудования** — UART (NS16550A), VirtIO block, PCI, PLIC
- **Парсер FDT** — обнаружение устройств через дерево устройств (память, PLIC, устройства)
- **Пер-процессная таблица FD** — 16 слотов на процесс с capability-токенами
- **Юзерленд** — `/bin/login`, `/bin/osh` (shell), `/bin/passwd`, `/bin/useradd`, `/bin/userdel`
- **Аутентификация** — `/etc/passwd` + `/etc/shadow`; интерактивная установка пароля root при первой загрузке
- **Файловая система /proc** — `version`, `cpuinfo`, `meminfo`, `uptime`, `load`, `stat`

----

## Архитектура

### Кольца привилегий

| Кольцо | Название | Режим | Описание |
|--------|----------|-------|----------|
| 0 | Ядро | S-mode | Полный доступ: память, CSR, прерывания, драйверы |
| 1 | Root Space | U-mode | PID 1 (`/bin/init`); все syscall; может `spawn`, `create`, `snapshot` |
| 2 | User Space | U-mode | Ограниченный ACL syscall; нет доступа к `/service/*` |

- `dropring(target)` — однонаправленное понижение привилегий: 0→1, 0→2, 1→2. Повышение привилегий невозможно.
- `/bin/login` понижает привилегии с ring 1 до ring 2, затем выполняет `exec` `/bin/osh`.

### Раскладка памяти (Sv39)

```
0x0000_0000_0000_0000 ──────────────────────
                         (не используется)
0x0000_0000_0001_0000  user_base
                         ┌─────────────────────┐
                         │  сегменты кода/данных│
                         ├─────────────────────┤
0x0000_0000_0100_0000  heap_base
                         │  куча пользователя  │  (~252 МБ через sbrk)
                         ├─────────────────────┤
0x0000_0000_2000_0000  ustack_top
                         │  стек пользователя  │  (16 страниц, 64 КБ)
0x0000_0000_4000_0000 ──────────────────────
                         (identity-маппинг ядра, первые 3 ГБ)
```

### Цепочка загрузки

1. **OnyxBoot** (M-mode) — загружает `kernel.elf` с VirtIO/SDHCI, передаёт управление в точку входа
2. **boot.S** (`kernel/src/arch/asm/boot.rs`) — обнуляет BSS, настраивает PMP, делегирует исключения в S-mode, `mret` → `kmain`
3. **kmain** (`kernel/src/srv/main.rs`) — UART, FDT, PMM, VMM, куча, traps, PLIC, VirtIO, монтирование VFS, загрузка `/bin/init`, `enter_user(1)`

### Жизненный цикл процесса

```
alloc_proc() → Ready → Running → Exited
                  ↑         ↓
                  └── sched_yield()
                          
Running ──wait()──→ Waiting ──поток вышел──→ Ready
Running ──kill()──→ Exited (SIGKILL)
```

### Бинарный формат OnyxExec v2

```
[0..3]   MAGIC "ONX\0"
[4..7]   flags (бит 0 = RING1, бит 1 = COMPRESSED)
[8..11]  смещение точки входа
[12..15] num_segments
[16..23] entry_vaddr
──────── seg[0..num_segments] (по 32 байта) ────────
  [0..7]   vaddr
  [8..11]  filesz
  [12..15] memsz
  [16..19] offset (в файле)
  [20..23] flags (VMM_R, VMM_W, VMM_X)
  [24..27] compressed_size (0 = без сжатия)
──────── сырые данные сегментов ────────
```

### Структура директорий

```
/bin/        — исполняемые файлы (init, login, osh, passwd, useradd, userdel, hello)
/service/    — root-сервисы (запускаются init-ом)
/etc/        — конфигурация (passwd, shadow)
/proc/       — информация о системе (version, cpuinfo, meminfo, uptime, load, stat)
/font/       — шрифты (default.psf)
/users/      — домашние директории пользователей (/users/username/)
```

----

## Сборка

### Зависимости

| Инструмент | Версия | Назначение |
|------------|--------|------------|
| Rust nightly | ≥ 1.85 | Компиляция ядра и юзерленда |
| `riscv64-elf-gcc` | — | Кросс-компиляция OnyxBoot |
| `qemu-system-riscv64` | — | Тестирование (опционально) |
| `parted`, `mkfs.fat`, `mcopy` | — | Создание загрузочного образа диска |
| `make` | — | Сборка OnyxBoot |

### Установка Rust-таргета

```console
$ rustup target add riscv64gc-unknown-none-elf
```

### Сборка ядра

```console
$ cargo build --release --target riscv64gc-unknown-none-elf
```

### Полная сборка + QEMU (одной командой)

```console
$ ./scripts/run_qemu.sh
```

Этот скрипт:
1. Собирает загрузчик **OnyxBoot** (`make -C ../OnyxBoot`)
2. Собирает **OnyxKernel**, **init** и **инструменты** (`elf2onx`, `mkimage`, `psfgen`)
3. Конвертирует ELF-бинарники юзерленда → `.onx` через `elf2onx`
4. Генерирует PSF1-шрифт через `psfgen`
5. Создаёт `disk.img` (OnyxFS) из манифеста через `mkimage`
6. Создаёт разделённый `boot.img` (FAT32 + OnyxFS)
7. Запускает QEMU

### Переопределение пути к OnyxBoot

```console
$ ONYXBOOT_DIR=/путь/к/OnyxBoot ./scripts/run_qemu.sh
```

----

## Запуск в QEMU

```console
$ qemu-system-riscv64 \
    -M virt -m 256M -smp 2 \
    -bios /путь/к/OnyxBoot/bootloader.bin \
    -drive file=build/boot.img,format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -nographic -no-reboot
```

Ожидаемый вывод:

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

## Структура проекта

| Путь | Описание |
|------|----------|
| `kernel/` | Ядро: arch, proc, mm, fs, drivers, syscall, ipc |
| `core/` | Общая библиотека: форматы OnyxExec/OnyxFS, парсер, errno |
| `init/` | Бинарники юзерленда: init, login, osh, passwd, useradd, userdel |
| `tools/` | Инструменты хоста: `elf2onx`, `mkimage`, `psfgen` |
| `scripts/` | Скрипты сборки и запуска (`run_qemu.sh`) |
| `docs/` | Документация по архитектуре |

### Карта модулей ядра

| Модуль | Путь | Описание |
|--------|------|----------|
| `arch` | `kernel/src/arch/` | Загрузка RISC-V, трапы, CSR, SMP, определения регистров |
| `proc` | `kernel/src/proc/` | Управление процессами, планировщик, spawn, сигналы |
| `mm` | `kernel/src/mm/` | PMM (bitmap + slab), VMM (Sv39), куча ядра |
| `fs/onyxfs` | `kernel/src/fs/onyxfs/` | OnyxFS v2: чтение, запись, mkdir, журнал, снэпшоты |
| `fs/vfs` | `kernel/src/fs/vfs/` | Виртуальная ФС: таблица монтирования, FD-токены, open/close/read/write |
| `fs/procfs` | `kernel/src/fs/procfs/` | ФС /proc: version, cpuinfo, meminfo, uptime, load, stat |
| `fs/ipcfs` | `kernel/src/fs/ipcfs/` | Записи /ipc/* VFS для именованных IPC-каналов |
| `drivers` | `kernel/src/drivers/` | UART, VirtIO block, PCI, PLIC, фреймбуфер, fb_term |
| `syscall` | `kernel/src/syscall/` | Syscall ABI, обработчик, ACL, диспетчеризация |
| `ipc` | `kernel/src/ipc/` | IPC каналы: create, connect, send, recv, close |
| `srv` | `kernel/src/srv/` | kmain, обработчик трапов, таймер, журнал ядра |
| `font` | `kernel/src/font/` | Парсер шрифтов PSF1/PSF2 и рендерер глифов |
| `libfdt` | `kernel/src/libfdt/` | Парсер Flattened Device Tree |

----

## Связанные проекты

| Проект | Описание |
|--------|----------|
| [OnyxBoot](https://github.com/anomalyco/OnyxBoot) | Минималистичный RISC-V 64-bit загрузчик (C++20, ~9.5 КБ) |
| [OnyxCompiller](https://github.com/anomalyco/OnyxCompiller) | Компилятор C → RV64 (работает на OnyxOS как `/bin/onyxcc`) |

----

## Дорожная карта

- [ ] Поддержка Unicode-таблицы в шрифтах PSF1/PSF2
- [ ] Именованные IPC-каналы через `/ipc/*` VFS
- [ ] Обнаружение оборудования через FDT (замена захардкоженных адресов)
- [ ] Диспетчеризация прерываний PLIC (сейчас только лог)
- [ ] Улучшения драйвера фреймбуфера
- [ ] Драйвер SDHCI (для Milk-V Duo S)
- [ ] Вывод `elf2onx` v2 (поле compressed_size)
- [ ] `mkimage` v2 (область снэпшотов + журнал в образе)
- [ ] Восстановление после паники / kdump
- [ ] Многоядерность (SMP)

----

## Лицензия

GPL-3.0-or-later. См. [LICENSE](LICENSE).
