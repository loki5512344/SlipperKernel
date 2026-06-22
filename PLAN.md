# SlipperKernel — Design & Roadmap

## Чекпоинты (Checkpoints)

### ☑️ CP-01: Boot + Console
- [x] SlipperBoot загружает kernel.elf через FAT32 (MBR partitioned)
- [x] M→S transition: PMP, delegation, mret
- [x] UART: NS16550A, FDT-driven
- [x] Banner «SLIPPER» + fastfetch + braille seal

### ☑️ CP-02: Memory
- [x] PMM: bitmap 4K alloc/free/alloc_zero
- [x] VMM: Sv39 identity map (1GB huge pages + split)
- [x] Heap: bump + free-list (kmalloc/kfree)
- [x] User space: 64KB stack + 64KB heap pre-allocated

### ☑️ CP-03: Traps + Syscalls
- [x] Trap handler: stvec, save/restore, sret
- [x] Scheduler: round-robin, 100Hz timer
- [x] Syscalls: write/read/exit/yield/open/close/lseek/stat/exec/sbrk
- [x] Line discipline: echo, backspace, enter

### ☑️ CP-04: Storage
- [x] virtio-blk: legacy v1 + modern v2, polled I/O
- [x] VFS layer
- [x] SlipperFS read-only (4096B blocks, bitmap inodes)
- [x] FAT32 read-only

### ☑️ CP-05: Userspace
- [x] SPX binary format (344B header, 40B segments)
- [x] spx_load: parse, map, copy, zero BSS
- [x] SYS_exec: load .spx from VFS, replace process
- [x] Init shell: C program (help, echo, cat, exec, clear, exit)
- [x] Rust core: memcpy/memset/strcmp (via core library)

### ⏳ CP-06: Filesystem write
- [ ] SlipperFS write/create/delete
- [ ] Directory listing syscall
- [ ] Множество SPX файлов на диске

### ⏳ CP-07: Networking
- [ ] VirtIO net driver
- [ ] LwIP or custom tiny stack
- [ ] TFTP boot

### ⏳ CP-08: Real hardware
- [ ] Milk-V Duo S / SG2002
- [ ] SDHCI driver in kernel
- [ ] GPIO / I2C / SPI

### ⏳ CP-09: Stable
- [ ] Initramfs (embed init in .rodata)
- [ ] SPX toolchain (compiler, linker, loader)

---

## Ключевые решения

### SPX вместо ELF
- 344 байта заголовок, 40 байт сегмент
- Никаких релокаций, GOT, динамической линковки
- Парсер: 100 строк, загрузка в 10 раз быстрее ELF

### Нет fork() — только exec()
- `SYS_exec(12)` — загружает SPX, заменяет текущий процесс
- Не нужно COW, копирование страниц

### Нет mmap/brk — предвыделенная куча
- 64KB куча маппится при spx_load
- `SYS_sbrk(13)` — двигает указатель в пределах кучи
- Никаких page fault при первом доступе

### Round-robin без preemption
- 4 слота, 100Hz tick
- Никаких priority inversion, tail-latency

### Нет модели драйверов Linux
- FDT → MMIO адрес → функция. Без module, platform_driver, deferred probe
- Инициализация за микросекунды

### Нет контейнеров / SELinux / AppArmor
- Три кольца RISC-V (M/S/U) + PMP + Sv39 U-бит
- 100 строк кода изоляции в boot.S

### Тулзы на C, не на Python
- elf2spx.c, mkimage.c — статические бинарники
- 0 зависимостей, компиляция за 0.1с

---

## SlipperFS — собственная ФС

| Блок | Назначение |
|------|-----------|
| 0 | Superblock (magic, version, размеры) |
| 1 | Inode bitmap (32 inode) |
| 2 | Data bitmap |
| 3 | Inode table (32 × 64 байта = 2KB) |
| 4+ | Data blocks |

- Блоки 4096 байт
- Inode: 64 байта, 10 прямых блоков + indirect (TODO)
- Имена до 32 байт
- ~200 строк кода

Почему своя, не FAT32/ext4: полный контроль, минимальный код, GPL-3.0 чистая,
легко расширять.
