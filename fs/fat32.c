// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * SlipperKernel — FAT32 read-only driver.
 */
#include "types.h"
#include "fat32.h"
#include "virtio.h"
#include "klog.h"

/* ------------------------------------------------------------------ */
/*  BPB / global state                                                */
/* ------------------------------------------------------------------ */
static int  g_dev            = -1;
static u16  g_bps;            /* bytes per sector          (BPB+0x0B) */
static u8   g_spc;            /* sectors per cluster       (BPB+0x0D) */
static u16  g_resvd;          /* reserved sectors          (BPB+0x0E) */
static u8   g_num_fats;       /* number of FATs            (BPB+0x10) */
static u32  g_fat_sz;         /* sectors per FAT           (BPB+0x24) */
static u32  g_root_cluster;   /* first cluster of root dir (BPB+0x2C) */
static u32  g_data_lba;       /* first LBA of data region              */

static u8 g_sec[512] __attribute__((aligned(8)));

/* ------------------------------------------------------------------ */
/*  Low-level helpers                                                  */
/* ------------------------------------------------------------------ */
static int read_sec(u32 lba, void *buf)
{
    return virtio_blk_read(g_dev, (u64)lba, buf);
}

static u32 cluster_to_lba(u32 cluster)
{
    return g_data_lba + (cluster - 2) * (u32)g_spc;
}

/* Read the FAT entry for `cluster` and return the next cluster.
 * Returns a value >= 0x0FFFFFF8 on EOC or error.                     */
static u32 fat_next(u32 cluster)
{
    u32 fat_off = cluster * 4;
    u32 fat_sec = (u32)g_resvd + fat_off / 512;
    u32 sec_off = fat_off % 512;

    if (read_sec(fat_sec, g_sec) < 0)
        return 0x0FFFFFFF;

    u32 val = *(u32 *)(g_sec + sec_off);
    return val & 0x0FFFFFFF;
}

/* ------------------------------------------------------------------ */
/*  Convert a filename to 8.3 format (upper-case, space-padded).      */
/* ------------------------------------------------------------------ */
static void name_to_83(const char *path, char out[11])
{
    int i;

    for (i = 0; i < 11; ++i)
        out[i] = ' ';

    /* locate last dot */
    const char *dot = NULL;
    for (const char *p = path; *p; ++p)
        if (*p == '.') dot = p;

    if (dot) {
        /* name part */
        for (i = 0; i < (int)(dot - path) && i < 8; ++i) {
            char c = path[i];
            if (c >= 'a' && c <= 'z') c -= 32;
            out[i] = c;
        }
        /* extension part */
        const char *ext = dot + 1;
        for (i = 0; i < 3 && ext[i]; ++i) {
            char c = ext[i];
            if (c >= 'a' && c <= 'z') c -= 32;
            out[8 + i] = c;
        }
    } else {
        /* no dot – treat as raw 11-byte (or less) 8.3 name */
        int len = 0;
        for (; path[len]; ++len);
        for (i = 0; i < len && i < 11; ++i) {
            char c = path[i];
            if (c >= 'a' && c <= 'z') c -= 32;
            out[i] = c;
        }
    }
}

/* ------------------------------------------------------------------ */
/*  fat32_mount – read BPB, verify, populate globals.                  */
/* ------------------------------------------------------------------ */
int fat32_mount(int virtio_dev)
{
    g_dev = virtio_dev;

    if (read_sec(0, g_sec) < 0) {
        kerr("fat32: cannot read BPB");
        return SL_ERR_IO;
    }

    if (g_sec[510] != 0x55 || g_sec[511] != 0xAA) {
        kerr("fat32: no boot signature");
        return SL_ERR_INVAL;
    }

    g_bps = *(u16 *)(g_sec + 0x0B);
    if (g_bps != 512) {
        kerr("fat32: unsupported bytes_per_sector=%u", g_bps);
        return SL_ERR_INVAL;
    }

    g_spc = *(u8 *)(g_sec + 0x0D);
    if (g_spc == 0 || (g_spc & (g_spc - 1)) != 0) {
        kerr("fat32: invalid sectors_per_cluster=%u", g_spc);
        return SL_ERR_INVAL;
    }

    g_resvd    = *(u16 *)(g_sec + 0x0E);
    g_num_fats = *(u8 *)(g_sec + 0x10);

    /* FAT32-specific extended BPB fields */
    g_fat_sz       = *(u32 *)(g_sec + 0x24);
    g_root_cluster = *(u32 *)(g_sec + 0x2C);

    g_data_lba = (u32)g_resvd + (u32)g_num_fats * g_fat_sz;

    kinf("fat32: mounted, spc=%u fats=%u fat_sz=%u root_cluster=%u",
         g_spc, g_num_fats, g_fat_sz, g_root_cluster);

    return 0;
}

/* ------------------------------------------------------------------ */
/*  fat32_lookup – find a file in the root directory.                  */
/* ------------------------------------------------------------------ */
int fat32_lookup(const char *path, u32 *out_cluster, u32 *out_size)
{
    char name83[11];
    u32  cluster;

    if (path[0] == 0)
        return SL_ERR_NOENT;

    name_to_83(path, name83);

    cluster = g_root_cluster;

    while (cluster >= 2 && cluster < 0x0FFFFFF8) {
        u32 lba = cluster_to_lba(cluster);

        for (u32 s = 0; s < (u32)g_spc; ++s) {
            if (read_sec(lba + s, g_sec) < 0)
                return SL_ERR_IO;

            /* 16 × 32-byte directory entries per 512-byte sector */
            for (int i = 0; i < 16; ++i) {
                u8 *ent = g_sec + i * 32;

                /* 0x00 = end of directory, 0xE5 = deleted */
                if (ent[0] == 0x00)
                    return SL_ERR_NOENT;
                if (ent[0] == 0xE5)
                    continue;

                u8 attr = ent[0x0B];
                if (attr == 0x0F)          /* LFN entry */
                    continue;
                if (attr & 0x08)           /* volume label */
                    continue;

                /* compare 8.3 short name */
                bool match = true;
                for (int j = 0; j < 11; ++j) {
                    if (ent[j] != (u8)name83[j]) {
                        match = false;
                        break;
                    }
                }
                if (!match)
                    continue;

                /* found */
                u32 hi = *(u16 *)(ent + 0x14);
                u32 lo = *(u16 *)(ent + 0x1A);
                *out_cluster = (hi << 16) | lo;
                *out_size    = *(u32 *)(ent + 0x1C);
                return 0;
            }
        }

        cluster = fat_next(cluster);
    }

    return SL_ERR_NOENT;
}

/* ------------------------------------------------------------------ */
/*  fat32_read – sequential read from a cluster chain.                 */
/* ------------------------------------------------------------------ */
int fat32_read(u32 cluster, void *buf, u32 off, u32 len)
{
    u8  *out = (u8 *)buf;
    u32  copied = 0;
    u32  clus_size = (u32)g_spc * 512;
    u32  clus = cluster;
    u32  clus_off = off;

    if (len == 0)
        return 0;

    /* walk cluster chain to find the cluster containing `off` */
    while (clus_off >= clus_size) {
        clus = fat_next(clus);
        if (clus >= 0x0FFFFFF8)
            return 0;
        clus_off -= clus_size;
    }

    /* read data */
    while (copied < len) {
        if (clus >= 0x0FFFFFF8)
            break;

        u32  lba   = cluster_to_lba(clus);
        u32  chunk = MIN(clus_size - clus_off, len - copied);
        u32  sec   = clus_off / 512;
        u32  sec_off = clus_off % 512;
        u32  remain  = chunk;

        while (remain > 0 && sec < (u32)g_spc) {
            u32 n = MIN(512 - sec_off, remain);

            if (read_sec(lba + sec, g_sec) < 0)
                return (int)copied;

            for (u32 i = 0; i < n; ++i)
                out[copied + i] = g_sec[sec_off + i];

            copied += n;
            remain -= n;
            ++sec;
            sec_off = 0;
        }

        clus_off = 0;
        clus = fat_next(clus);
    }

    return (int)copied;
}
