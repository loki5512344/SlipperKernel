CROSS    ?= riscv64-elf-
CC       := $(CROSS)gcc
OBJCOPY  := $(CROSS)objcopy
OBJDUMP  := $(CROSS)objdump
HOST_CC  ?= gcc
CARGO    ?= cargo

ARCH     := -march=rv64gc -mabi=lp64d -mcmodel=medany

CFLAGS   := $(ARCH) -std=gnu17 -ffreestanding -nostdlib -nostartfiles \
            -fno-pic -fno-pie -fno-stack-protector \
            -fno-exceptions -fno-asynchronous-unwind-tables \
            -O2 -g -Wall -Wextra -Wno-unused-parameter \
            -DKLOG_LEVEL=3

INCLUDES := -I include

LDFLAGS  := -Wl,-T,arch/riscv64/linker.ld -Wl,--gc-sections -nostdlib -nostartfiles

BUILD    := build

RUST_DIR := rust
RUST_LIB := $(RUST_DIR)/target/riscv64gc-unknown-none-elf/release/liblibslipper_core.a

ASM_SRC  := arch/riscv64/boot.S arch/riscv64/trap.S
C_SRC    := \
    kernel/main.c     \
    kernel/klog.c     \
    kernel/trap.c     \
    kernel/timer.c    \
    kernel/syscall.c  \
    proc/proc.c       \
    proc/spx.c        \
    mm/pmm.c          \
    mm/vmm.c          \
    mm/vmm_map.c      \
    mm/heap.c         \
    fs/vfs.c          \
    fs/slipperfs.c    \
    fs/fat32.c        \
    drivers/uart.c    \
    drivers/virtio.c  \
    drivers/virtio_req.c \
    drivers/plic.c \
    lib/fdt.c         \
    lib/fdt_find.c    \
    lib/string.c

OBJ      := $(patsubst %.S,$(BUILD)/%.o,$(ASM_SRC)) \
            $(patsubst %.c,$(BUILD)/%.o,$(C_SRC))

KERNEL   := $(BUILD)/kernel.elf

HOST_TOOLS := $(BUILD)/elf2spx $(BUILD)/mkimage

.PHONY: all clean run docs rust

all: $(KERNEL) $(BUILD)/init.spx $(BUILD)/disk.img

rust:
	$(CARGO) build --release --target riscv64gc-unknown-none-elf --manifest-path $(RUST_DIR)/Cargo.toml

$(RUST_LIB): rust

$(KERNEL): $(OBJ) $(RUST_LIB) arch/riscv64/linker.ld
	@mkdir -p $(BUILD)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $(OBJ) -L $(RUST_DIR)/target/riscv64gc-unknown-none-elf/release -l slipper_core
	@echo "  [LD] $@"
	@$(OBJDUMP) -h $@ | head -20

$(BUILD)/%.o: %.S
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(INCLUDES) -c -o $@ $<
	@echo "  [AS] $<"

$(BUILD)/%.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(INCLUDES) -c -o $@ $<
	@echo "  [CC] $<"

$(BUILD)/elf2spx: scripts/elf2spx.c
	@mkdir -p $(BUILD)
	$(HOST_CC) -O2 -o $@ $<
	@echo "  [HOST_CC] $<"

$(BUILD)/mkimage: scripts/mkimage.c
	@mkdir -p $(BUILD)
	$(HOST_CC) -O2 -o $@ $<
	@echo "  [HOST_CC] $<"

INIT_ELF := $(BUILD)/init.elf
INIT_SPX := $(BUILD)/init.spx

$(INIT_ELF): init/init.c init/init.ld
	@mkdir -p $(BUILD)
	$(CC) $(ARCH) -ffreestanding -nostdlib -nostartfiles -static \
	    -Wl,-T,init/init.ld -o $@ init/init.c
	@echo "  [INIT] $@"

$(INIT_SPX): $(INIT_ELF) $(BUILD)/elf2spx
	$(BUILD)/elf2spx $< $@
	@echo "  [SPX] $@"

DISK_IMG := $(BUILD)/disk.img

$(DISK_IMG): $(INIT_SPX) $(BUILD)/mkimage
	$(BUILD)/mkimage $< $@
	@echo "  [IMG] $@"

run: all
	bash scripts/run_qemu.sh

docs:
	@echo "docs target: not implemented"

clean:
	rm -rf $(BUILD)
	$(CARGO) clean --manifest-path $(RUST_DIR)/Cargo.toml
