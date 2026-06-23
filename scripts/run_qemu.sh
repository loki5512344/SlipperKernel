#!/bin/bash
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
BOOT_DIR="${ONYXBOOT_DIR:-/home/z/my-project/OnyxBoot}"
RISCV_TOOLS="${RISCV_TOOLS:-/home/z/my-project/riscv-tools/usr/bin}"
export PATH="$RISCV_TOOLS:$HOME/.cargo/bin:$PATH"
export LD_LIBRARY_PATH="/home/z/my-project/riscv-tools/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"

# Build OnyxBoot if needed
if [ ! -f "$BOOT_DIR/bootloader.bin" ]; then
    echo "==> Building OnyxBoot"
    make -C "$BOOT_DIR" CROSS=riscv64-linux-gnu clean all 2>&1 | tail -3
fi

# Build OnyxKernel + init + tools
echo "==> Building OnyxKernel"
cd "$ROOT"
. "$HOME/.cargo/env"
cargo build --release -p onyx_kernel --target riscv64gc-unknown-none-elf 2>&1 | tail -3
cargo build --release -p onyx_init --target riscv64gc-unknown-none-elf 2>&1 | tail -3
cargo build --release -p onyx_tools 2>&1 | tail -3

# Convert all userland ELFs to .onx
BUILD="$ROOT/build"
mkdir -p "$BUILD"
echo "==> Converting userland ELFs → .onx"
"$ROOT/target/release/elf2onx" --ring=1 "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-init" "$BUILD/init.onx"
"$ROOT/target/release/elf2onx" --ring=1 "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-login" "$BUILD/login.onx"
"$ROOT/target/release/elf2onx" "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-osh" "$BUILD/osh.onx"
"$ROOT/target/release/elf2onx" "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-passwd" "$BUILD/passwd.onx"
"$ROOT/target/release/elf2onx" --ring=1 "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-useradd" "$BUILD/useradd.onx"
"$ROOT/target/release/elf2onx" --ring=1 "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-userdel" "$BUILD/userdel.onx"

# No default passwd/shadow — first boot creates them interactively.

# Create manifest
cat > "$BUILD/manifest.txt" << EOF
dir /bin
dir /etc
dir /service
dir /users
dir /font
file $BUILD/init.onx /bin/init --ring=1
file $BUILD/login.onx /bin/login --ring=1
file $BUILD/osh.onx /bin/osh
file $BUILD/passwd.onx /bin/passwd
file $BUILD/useradd.onx /bin/useradd --ring=1
file $BUILD/userdel.onx /bin/userdel --ring=1
EOF

# Create OnyxFS disk image using manifest
echo "==> Creating OnyxFS disk image"
"$ROOT/target/release/mkimage" "$BUILD/manifest.txt" "$BUILD/disk.img"

# Create partitioned boot disk
echo "==> Creating partitioned boot disk"
FAT_LBA=2048
dd if=/dev/zero of="$BUILD/boot.img" bs=1M count=64 2>/dev/null
parted -s "$BUILD/boot.img" mklabel msdos 2>/dev/null
parted -s "$BUILD/boot.img" mkpart primary fat32 1MiB 5MiB 2>/dev/null
mkfs.fat -F 32 "$BUILD/boot.img" --offset=$FAT_LBA 2>/dev/null
mcopy -i "$BUILD/boot.img@@$((FAT_LBA * 512))" "$ROOT/target/riscv64gc-unknown-none-elf/release/onyx-kernel" ::kernel.elf 2>/dev/null
SLBA=10240
dd if="$BUILD/disk.img" of="$BUILD/boot.img" bs=512 seek=$SLBA conv=notrunc 2>/dev/null

echo "==> Starting QEMU"
# No timeout — interactive first-boot setup requires user input.
qemu-system-riscv64 \
    -M virt -m 256M \
    -bios "$BOOT_DIR/bootloader.bin" \
    -drive file="$BUILD/boot.img",format=raw,if=none,id=drive0 \
    -device virtio-blk-device,drive=drive0 \
    -display none -serial mon:stdio -no-reboot
