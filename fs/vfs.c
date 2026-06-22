// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — VFS layer.
 */
#include "types.h"
#include "vfs.h"
#include "slipperfs.h"
#include "fat32.h"
#include "heap.h"
#include "klog.h"
#include "riscv.h"

static vfs_fs_t g_root_fs = VFS_FS_NONE;
static vfs_fd_t g_fds[VFS_MAX_FDS];

int vfs_init(void)
{
    for (int i = 0; i < VFS_MAX_FDS; ++i) {
        g_fds[i].used = 0;
    }
    g_root_fs = VFS_FS_NONE;
    return 0;
}

int vfs_mount_root(int virtio_dev, u64 slipperfs_lba)
{
    if (spfs_mount(virtio_dev, slipperfs_lba) == 0) {
        g_root_fs = VFS_FS_SLIPPER;
        kinf("vfs: root mounted (SlipperFS)");
        return 0;
    }
    if (fat32_mount(virtio_dev) == 0) {
        g_root_fs = VFS_FS_FAT32;
        kinf("vfs: root mounted (FAT32)");
        return 0;
    }
    kerr("vfs: no filesystem recognised on dev %d", virtio_dev);
    return SL_ERR_NOSYS;
}

static int alloc_fd(void)
{
    for (int i = 0; i < VFS_MAX_FDS; ++i)
        if (!g_fds[i].used) return i;
    return SL_ERR_BUSY;
}

int vfs_open(const char *path)
{
    if (path[0] != '/') return SL_ERR_INVAL;
    /* Skip leading '/'. */
    const char *name = path + 1;

    int fd = alloc_fd();
    if (fd < 0) return fd;
    vfs_fd_t *f = &g_fds[fd];

    if (g_root_fs == VFS_FS_SLIPPER) {
        spfs_stat_t st;
        int rc = spfs_lookup(name, &st);
        if (rc) return rc;
        f->ino  = st.ino;
        f->size = st.size;
        f->pos  = 0;
        f->fs   = VFS_FS_SLIPPER;
        f->used = 1;
        return fd;
    }
    if (g_root_fs == VFS_FS_FAT32) {
        u32 cluster, size;
        int rc = fat32_lookup(name, &cluster, &size);
        if (rc) return rc;
        f->ino  = cluster;
        f->size = size;
        f->pos  = 0;
        f->fs   = VFS_FS_FAT32;
        f->used = 1;
        return fd;
    }
    return SL_ERR_NOSYS;
}

int vfs_read(int fd, void *buf, usize len)
{
    if (fd < 0 || fd >= VFS_MAX_FDS) return SL_ERR_INVAL;
    vfs_fd_t *f = &g_fds[fd];
    if (!f->used) return SL_ERR_INVAL;
    if (f->fs == VFS_FS_SLIPPER) {
        int n = spfs_read(f->ino, buf, f->pos, (u32)len);
        if (n > 0) f->pos += (u32)n;
        return n;
    }
    if (f->fs == VFS_FS_FAT32) {
        if (f->pos >= f->size) return 0;
        u32 read_len = MIN((u32)len, f->size - f->pos);
        int n = fat32_read(f->ino, buf, f->pos, read_len);
        if (n > 0) f->pos += (u32)n;
        return n;
    }
    return SL_ERR_NOSYS;
}

int vfs_close(int fd)
{
    if (fd < 0 || fd >= VFS_MAX_FDS) return SL_ERR_INVAL;
    g_fds[fd].used = 0;
    return 0;
}

int vfs_stat(int fd, u32 *size_out)
{
    if (fd < 0 || fd >= VFS_MAX_FDS) return SL_ERR_INVAL;
    if (!g_fds[fd].used) return SL_ERR_INVAL;
    if (size_out) *size_out = g_fds[fd].size;
    return 0;
}

vfs_fd_t *vfs_get_fd(int fd)
{
    if (fd < 0 || fd >= VFS_MAX_FDS) return NULL;
    if (!g_fds[fd].used) return NULL;
    return &g_fds[fd];
}
