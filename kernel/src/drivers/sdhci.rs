//! SDHCI (Secure Digital Host Controller Interface) driver.
//!
//! Supports the QEMU virt platform's SDHCI emulation at 0x10800000 and
//! the Milk-V Duo S board. Implements the standard SD card initialization
//! sequence (CMD0 → CMD8 → ACMD41 → CMD2 → CMD3 → CMD9 → CMD7 → CMD16)
//! and block read/write operations using 512-byte sectors.
//!
//! Integrates with the existing block device abstraction (same interface
//! as virtio-blk) and supports FDT-based device discovery.
use crate::arch::mmio::Mmio;
use crate::drivers::plic;

// ── SDHCI register offsets ──────────────────────────────────────────────
const SDMAS_SYS_ADDR: u32 = 0x00;
const BLOCK_SIZE: u32 = 0x04;
const BLOCK_COUNT: u32 = 0x06;
const ARGUMENT: u32 = 0x08;
const TRANSFER_MODE: u32 = 0x0C;
const COMMAND: u32 = 0x0E;
const RESPONSE0: u32 = 0x10;
const _RESPONSE1: u32 = 0x14;
const _RESPONSE2: u32 = 0x18;
const _RESPONSE3: u32 = 0x1C;
const BUFFER_DATA: u32 = 0x20;
const PRESENT_STATE: u32 = 0x24;
const HOST_CONTROL: u32 = 0x28;
const POWER_CONTROL: u32 = 0x2C;
const CLOCK_CONTROL: u32 = 0x30;
const TIMEOUT_CONTROL: u32 = 0x34;
const SOFTWARE_RESET: u32 = 0x38;
const NORMAL_INT_STATUS: u32 = 0x3C;
const ERROR_INT_STATUS: u32 = 0x40;
const NORMAL_INT_STATUS_ENABLE: u32 = 0x44;
const ERROR_INT_STATUS_ENABLE: u32 = 0x48;
const _AUTO_CMD_ERROR: u32 = 0x50;

// ── Present State bits ─────────────────────────────────────────────────
const PS_CMD_INHIBIT: u32 = 1 << 0;
const PS_DAT_INHIBIT: u32 = 1 << 1;
const _PS_DAT_LINE_ACTIVE: u32 = 1 << 2;
const _PS_WRITE_TRANSFER: u32 = 1 << 8;
const _PS_READ_TRANSFER: u32 = 1 << 9;
const PS_BUFFER_WRITE_ENABLE: u32 = 1 << 10;
const PS_BUFFER_READ_ENABLE: u32 = 1 << 11;
const PS_CARD_INSERTED: u32 = 1 << 16;
const PS_CARD_STATE_STABLE: u32 = 1 << 17;

// ── Command register bits ──────────────────────────────────────────────
const CMD_RESP_NONE: u16 = 0 << 0;
const CMD_RESP_136: u16 = 1 << 0;
const CMD_RESP_48: u16 = 2 << 0;
const CMD_RESP_48_BUSY: u16 = 3 << 0;
const CMD_CRC_ENABLE: u16 = 1 << 3;
const CMD_INDEX_ENABLE: u16 = 1 << 4;
const CMD_DATA_PRESENT: u16 = 1 << 5;

// ── Transfer Mode bits ─────────────────────────────────────────────────
const TM_READ: u16 = 1 << 4;
const _TM_BLOCK_COUNT: u16 = 1 << 1;

// ── Software Reset bits ────────────────────────────────────────────────
const SW_RESET_ALL: u32 = 1 << 0;
const SW_RESET_CMD: u32 = 1 << 1;
const SW_RESET_DAT: u32 = 1 << 2;

// ── Normal Interrupt Status bits ───────────────────────────────────────
const INT_CMD_COMPLETE: u32 = 1 << 0;
const INT_TRANSFER_COMPLETE: u32 = 1 << 1;
const INT_ERROR: u32 = 1 << 15;

// ── Error Interrupt Status bits ────────────────────────────────────────
const _ERR_CMD_TIMEOUT: u32 = 1 << 0;
const _ERR_CMD_CRC: u32 = 1 << 1;
const _ERR_DAT_TIMEOUT: u32 = 1 << 4;
const _ERR_DAT_CRC: u32 = 1 << 5;

// ── Clock Control bits ─────────────────────────────────────────────────
const CLK_INTERNAL_ENABLE: u32 = 1 << 0;
const CLK_STABLE: u32 = 1 << 1;
const CLK_SD_CLOCK_ENABLE: u32 = 1 << 2;
const CLK_MAX_DIV: u32 = 0x3FF;

// ── Power Control bits ─────────────────────────────────────────────────
const PWR_BUS_POWER: u32 = 1 << 0;
const PWR_3_3V: u32 = 7 << 1; // 3.3V selection

// ── Host Control bits ──────────────────────────────────────────────────
const HC_DATA_WIDTH_4BIT: u32 = 1 << 1;
const HC_HIGH_SPEED: u32 = 1 << 2;

// ── SD Command numbers ─────────────────────────────────────────────────
const CMD_GO_IDLE_STATE: u16 = 0;
const CMD_ALL_SEND_CID: u16 = 2;
const CMD_SEND_RELATIVE_ADDR: u16 = 3;
const _CMD_SET_DSR: u16 = 4;
const CMD_SELECT_CARD: u16 = 7;
const CMD_SEND_IF_COND: u16 = 8;
const CMD_SEND_CSD: u16 = 9;
const CMD_SET_BLOCKLEN: u16 = 16;
const CMD_READ_SINGLE_BLOCK: u16 = 17;
const CMD_READ_MULTIPLE_BLOCK: u16 = 18;
const CMD_WRITE_SINGLE_BLOCK: u16 = 24;
const CMD_WRITE_MULTIPLE_BLOCK: u16 = 25;
const CMD_APP_CMD: u16 = 55;
const ACMD_SD_SEND_OP_COND: u16 = 41;

// ── SDHCI default base address (QEMU virt) ─────────────────────────────
const _SDHCI_DEFAULT_BASE: usize = 0x1080_0000;
const SDHCI_SECTOR_SIZE: usize = 512;
const SDHCI_TIMEOUT: u32 = 1_000_000;

/// PLIC IRQ for SDHCI on QEMU virt. The QEMU virt machine typically
/// assigns IRQ 0x07 for the SDHCI controller, but this can be overridden
/// via FDT discovery.
pub const PLIC_PRIO_SDHCI: u32 = 0x07;

// ── Global SDHCI device state ──────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct SdhciDev {
    pub base: usize,
    pub rca: u32,          // Relative Card Address
    pub initialized: bool,
    pub irq: u32,
}

static mut G_SDHCI: SdhciDev = SdhciDev {
    base: 0,
    rca: 0,
    initialized: false,
    irq: PLIC_PRIO_SDHCI,
};

// ── MMIO register accessors ────────────────────────────────────────────

#[inline]
unsafe fn reg_r(base: usize, off: u32) -> u32 {
    Mmio::<u32>::at(base + off as usize).read()
}

#[inline]
unsafe fn reg_w(base: usize, off: u32, v: u32) {
    Mmio::<u32>::at(base + off as usize).write(v);
}

#[inline]
unsafe fn reg_r16(base: usize, off: u32) -> u16 {
    Mmio::<u16>::at(base + off as usize).read()
}

#[inline]
unsafe fn reg_w16(base: usize, off: u32, v: u16) {
    Mmio::<u16>::at(base + off as usize).write(v);
}

// ── Low-level SDHCI helpers ────────────────────────────────────────────

/// Wait for both CMD and DAT lines to be idle.
unsafe fn wait_idle(base: usize) {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let ps = reg_r(base, PRESENT_STATE);
        if (ps & (PS_CMD_INHIBIT | PS_DAT_INHIBIT)) == 0 {
            return;
        }
        timeout -= 1;
    }
}

/// Wait for a specific present-state flag to be set.
unsafe fn wait_state(base: usize, flag: u32) -> bool {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, PRESENT_STATE) & flag != 0 {
            return true;
        }
        timeout -= 1;
    }
    false
}

/// Software reset: all, CMD line, or DAT line.
unsafe fn reset(base: usize, bits: u32) {
    reg_w(base, SOFTWARE_RESET, bits);
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, SOFTWARE_RESET) & bits == 0 {
            return;
        }
        timeout -= 1;
    }
}

/// Clear all pending interrupt flags.
unsafe fn clear_interrupts(base: usize) {
    let norm = reg_r(base, NORMAL_INT_STATUS);
    reg_w(base, NORMAL_INT_STATUS, norm);
    let err = reg_r(base, ERROR_INT_STATUS);
    reg_w(base, ERROR_INT_STATUS, err);
}

/// Set up the SDHCI clock: divide the base clock and enable.
unsafe fn init_clock(base: usize) {
    // Read capabilities register to find base clock frequency.
    // Capabilities are at offset 0x40 (2nd half) and 0x44.
    // For QEMU, base clock is typically 200 MHz.
    // We target 50 MHz → divisor = 4 (base_clock / (2 * divisor)).
    // Capabilities register 0: offset 0x40
    //   bits 15:8 = base clock frequency in MHz
    //   bits 7:0  = timeout clock frequency
    let caps0 = reg_r(base, 0x40);
    let base_clk_mhz = ((caps0 >> 8) & 0xFF) as u32;
    let target_mhz: u32 = 50;

    let div: u32 = if base_clk_mhz > 0 && base_clk_mhz > target_mhz {
        // Clock divisor = base / (2 * target), round up
        let d = base_clk_mhz / (2 * target_mhz);
        if d == 0 { 1 } else { d.min(CLK_MAX_DIV) }
    } else {
        1
    };

    // Disable clock before changing
    reg_w(base, CLOCK_CONTROL, 0);

    // Set divisor and enable internal clock
    let clk_val = (div << 8) | CLK_INTERNAL_ENABLE;
    reg_w(base, CLOCK_CONTROL, clk_val);

    // Wait for clock stable
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        if reg_r(base, CLOCK_CONTROL) & CLK_STABLE != 0 {
            break;
        }
        timeout -= 1;
    }

    // Enable SD clock
    reg_w(base, CLOCK_CONTROL, clk_val | CLK_SD_CLOCK_ENABLE);
}

/// Enable power to the SD bus.
unsafe fn set_power(base: usize) {
    reg_w(base, POWER_CONTROL, PWR_3_3V);
    // Small delay for power ramp
    let mut delay = 1000u32;
    while delay > 0 {
        delay -= 1;
    }
    reg_w(base, POWER_CONTROL, PWR_3_3V | PWR_BUS_POWER);
}

/// Set the SDMA system address for DMA transfers.
unsafe fn set_sdma_addr(base: usize, addr: u64) {
    reg_w(base, SDMAS_SYS_ADDR, addr as u32);
}

/// Send an SD command and wait for completion.
/// Returns response[0] on success, or 0xFFFF_FFFF on timeout/error.
unsafe fn send_command(
    base: usize,
    cmd_idx: u16,
    arg: u32,
    resp_type: u16,
    has_data: bool,
) -> u32 {
    // Wait for CMD line to be idle
    wait_idle(base);

    // Clear pending interrupts
    clear_interrupts(base);

    // Reset CMD line if needed
    let ps = reg_r(base, PRESENT_STATE);
    if ps & PS_CMD_INHIBIT != 0 {
        reset(base, SW_RESET_CMD);
    }

    // Set up block size and block count for data commands
    if has_data {
        reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
        reg_w16(base, BLOCK_COUNT, 1);
    }

    // Write argument
    reg_w(base, ARGUMENT, arg);

    // Build command register
    let mut cmd_reg: u16 = (cmd_idx & 0x3F) << 8;
    cmd_reg |= resp_type;
    if has_data {
        cmd_reg |= CMD_DATA_PRESENT;
    }

    // For CMD_IDX 0/2/9, don't enable CRC/index checks
    if cmd_idx == CMD_GO_IDLE_STATE {
        // No response expected
    } else if cmd_idx == CMD_ALL_SEND_CID || cmd_idx == CMD_SEND_CSD {
        cmd_reg |= CMD_CRC_ENABLE;
        // Don't enable CMD_INDEX_ENABLE for long responses
    } else if cmd_idx == CMD_SEND_IF_COND || cmd_idx == CMD_APP_CMD {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SEND_RELATIVE_ADDR {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SELECT_CARD {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_SET_BLOCKLEN {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_READ_SINGLE_BLOCK || cmd_idx == CMD_READ_MULTIPLE_BLOCK {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    } else if cmd_idx == CMD_WRITE_SINGLE_BLOCK || cmd_idx == CMD_WRITE_MULTIPLE_BLOCK {
        cmd_reg |= CMD_CRC_ENABLE | CMD_INDEX_ENABLE;
    }

    // Write command register (triggers the command)
    reg_w16(base, COMMAND, cmd_reg);

    // Wait for Command Complete interrupt
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let istat = reg_r(base, NORMAL_INT_STATUS);
        if istat & INT_ERROR != 0 {
            // Error occurred — clear and return failure
            clear_interrupts(base);
            reset(base, SW_RESET_CMD);
            return 0xFFFF_FFFF;
        }
        if istat & INT_CMD_COMPLETE != 0 {
            // Clear the interrupt
            reg_w(base, NORMAL_INT_STATUS, INT_CMD_COMPLETE);
            break;
        }
        timeout -= 1;
    }
    if timeout == 0 {
        reset(base, SW_RESET_CMD);
        return 0xFFFF_FFFF;
    }

    // Read response
    reg_r(base, RESPONSE0)
}

/// Wait for transfer complete after a data operation.
unsafe fn wait_transfer_complete(base: usize) -> bool {
    let mut timeout = SDHCI_TIMEOUT;
    while timeout > 0 {
        let istat = reg_r(base, NORMAL_INT_STATUS);
        if istat & INT_ERROR != 0 {
            clear_interrupts(base);
            reset(base, SW_RESET_DAT);
            return false;
        }
        if istat & INT_TRANSFER_COMPLETE != 0 {
            reg_w(base, NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
            return true;
        }
        timeout -= 1;
    }
    reset(base, SW_RESET_DAT);
    false
}

// ── Public API ──────────────────────────────────────────────────────────

/// Probe for an SDHCI controller at the given base address.
/// Checks that the controller is present and a card is inserted.
pub unsafe fn probe(base: usize) -> bool {
    // Read the controller version (offset 0xFE, 2 bytes)
    // SDHCI spec: version register exists, non-zero means controller present
    let version = reg_r16(base, 0xFE);
    if version == 0 {
        return false;
    }
    // Check that the capabilities register looks sane
    let caps = reg_r(base, 0x40);
    // The slot type (bits 31:30) should be 0 (removable card)
    if (caps >> 30) & 0x3 != 0 {
        return false;
    }
    // Software reset the controller
    reset(base, SW_RESET_ALL);
    // Check card is present
    let ps = reg_r(base, PRESENT_STATE);
    ps & PS_CARD_INSERTED != 0
}

/// Initialize the SDHCI controller and the SD card.
/// Returns true on success.
pub unsafe fn init(base: usize, irq: u32) -> bool {
    // Full software reset
    reset(base, SW_RESET_ALL);

    // Enable all normal interrupt status bits
    reg_w(base, NORMAL_INT_STATUS_ENABLE, 0xFFFF);
    // Enable error interrupt status bits
    reg_w(base, ERROR_INT_STATUS_ENABLE, 0xFFFF);

    // Set up clocks
    init_clock(base);

    // Power on
    set_power(base);

    // Wait for card state to stabilize
    if !wait_state(base, PS_CARD_STATE_STABLE) {
        return false;
    }

    // Set timeout to maximum (0x0E = ~2.7s at timeout clock)
    reg_w(base, TIMEOUT_CONTROL, 0x0E);

    // ── SD card initialization sequence ─────────────────────────────

    // CMD0: GO_IDLE_STATE — reset card to idle
    send_command(base, CMD_GO_IDLE_STATE, 0, CMD_RESP_NONE, false);

    // Small delay after CMD0
    let mut delay = 100_000u32;
    while delay > 0 {
        delay -= 1;
    }

    // CMD8: SEND_IF_COND — verify SD v2.x and voltage
    // Argument: 0x000001AA = voltage 2.7-3.6V, check pattern 0xAA
    let resp8 = send_command(
        base,
        CMD_SEND_IF_COND,
        0x000001AA,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    let sd_v2 = resp8 != 0xFFFF_FFFF && (resp8 & 0xFF) == 0xAA;

    // ACMD41: SD_SEND_OP_COND — initialize card, wait for ready
    // ACMD41 is preceded by CMD55 (APP_CMD)
    let mut op_cond_ok = false;
    let mut retries = 2000u32;
    let ocr = if sd_v2 { 0x40FF_8000 } else { 0x00FF_8000 };
    while retries > 0 {
        // CMD55: APP_CMD (next command is application-specific)
        let rca55 = send_command(
            base,
            CMD_APP_CMD,
            0,
            CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
            false,
        );
        if rca55 == 0xFFFF_FFFF {
            retries -= 1;
            continue;
        }

        // ACMD41: SD_SEND_OP_COND
        let resp41 = send_command(
            base,
            ACMD_SD_SEND_OP_COND,
            ocr,
            CMD_RESP_48 | CMD_CRC_ENABLE,
            false,
        );
        // Bit 31 = card power-up status (busy) bit
        if resp41 != 0xFFFF_FFFF && (resp41 & (1 << 31)) != 0 {
            op_cond_ok = true;
            break;
        }
        retries -= 1;
    }
    if !op_cond_ok {
        return false;
    }

    // CMD2: ALL_SEND_CID — ask card to send CID
    send_command(
        base,
        CMD_ALL_SEND_CID,
        0,
        CMD_RESP_136 | CMD_CRC_ENABLE,
        false,
    );

    // CMD3: SEND_RELATIVE_ADDR — ask card to publish RCA
    let resp3 = send_command(
        base,
        CMD_SEND_RELATIVE_ADDR,
        0,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp3 == 0xFFFF_FFFF {
        return false;
    }
    let rca = resp3 >> 16;

    // CMD9: SEND_CSD — get card-specific data
    send_command(
        base,
        CMD_SEND_CSD,
        rca << 16,
        CMD_RESP_136 | CMD_CRC_ENABLE,
        false,
    );

    // CMD7: SELECT_CARD — select the card with the given RCA
    let resp7 = send_command(
        base,
        CMD_SELECT_CARD,
        rca << 16,
        CMD_RESP_48_BUSY | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp7 == 0xFFFF_FFFF {
        return false;
    }

    // CMD16: SET_BLOCKLEN — set block length to 512
    let resp16 = send_command(
        base,
        CMD_SET_BLOCKLEN,
        SDHCI_SECTOR_SIZE as u32,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        false,
    );
    if resp16 == 0xFFFF_FFFF {
        return false;
    }

    // Set 4-bit data width and high speed (optional, improves performance)
    reg_w(base, HOST_CONTROL, HC_DATA_WIDTH_4BIT | HC_HIGH_SPEED);

    // Store device state
    let p = &raw mut G_SDHCI;
    (*p).base = base;
    (*p).rca = rca;
    (*p).initialized = true;
    (*p).irq = irq;

    // Register PLIC interrupt handler
    plic::set_priority(irq, 5);
    plic::enable(irq, 0);

    true
}

/// Read a single 512-byte sector from the SD card.
/// `lba` is the logical block address (sector number).
/// `buf` must be 512 bytes, aligned to at least 4 bytes for SDMA.
pub unsafe fn read(lba: u64, buf: &mut [u8; 512]) -> bool {
    let p = &raw const G_SDHCI;
    if !(*p).initialized {
        return false;
    }
    let base = (*p).base;

    // Wait for idle
    wait_idle(base);

    // Set up block size with DMA boundary (512 = 0x200, DMA boundary = 4KiB)
    reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
    reg_w16(base, BLOCK_COUNT, 1);

    // Set SDMA address
    set_sdma_addr(base, buf.as_ptr() as u64);

    // Set transfer mode: read, single block
    reg_w16(base, TRANSFER_MODE, TM_READ);

    // Send CMD17: READ_SINGLE_BLOCK
    // SD card uses byte address (not sector) for standard capacity cards,
    // but SDHC/SDXC use sector address. We use sector address (lba).
    let resp = send_command(
        base,
        CMD_READ_SINGLE_BLOCK,
        lba as u32,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        true,
    );
    if resp == 0xFFFF_FFFF {
        return false;
    }

    // Wait for buffer read enable
    if !wait_state(base, PS_BUFFER_READ_ENABLE) {
        reset(base, SW_RESET_DAT);
        return false;
    }

    // Wait for transfer complete
    if !wait_transfer_complete(base) {
        return false;
    }

    // Read any remaining data from the buffer data port if SDMA didn't work
    // (fallback: PIO mode). Check if the buffer still has data.
    let ps = reg_r(base, PRESENT_STATE);
    if ps & PS_BUFFER_READ_ENABLE != 0 {
        // Read data from buffer port (32 bits at a time)
        for i in (0..512).step_by(4) {
            let word = reg_r(base, BUFFER_DATA);
            buf[i] = word as u8;
            buf[i + 1] = (word >> 8) as u8;
            buf[i + 2] = (word >> 16) as u8;
            buf[i + 3] = (word >> 24) as u8;
        }
    }

    true
}

/// Write a single 512-byte sector to the SD card.
/// `lba` is the logical block address (sector number).
/// `buf` must be 512 bytes, aligned to at least 4 bytes for SDMA.
pub unsafe fn write(lba: u64, buf: &[u8; 512]) -> bool {
    let p = &raw const G_SDHCI;
    if !(*p).initialized {
        return false;
    }
    let base = (*p).base;

    // Wait for idle
    wait_idle(base);

    // Set up block size and count
    reg_w16(base, BLOCK_SIZE, SDHCI_SECTOR_SIZE as u16);
    reg_w16(base, BLOCK_COUNT, 1);

    // Set SDMA address
    set_sdma_addr(base, buf.as_ptr() as u64);

    // Clear interrupts
    clear_interrupts(base);

    // Set transfer mode: write, single block
    reg_w16(base, TRANSFER_MODE, 0); // write = bit4 = 0

    // Send CMD24: WRITE_SINGLE_BLOCK
    let resp = send_command(
        base,
        CMD_WRITE_SINGLE_BLOCK,
        lba as u32,
        CMD_RESP_48 | CMD_CRC_ENABLE | CMD_INDEX_ENABLE,
        true,
    );
    if resp == 0xFFFF_FFFF {
        return false;
    }

    // Wait for buffer write enable
    if !wait_state(base, PS_BUFFER_WRITE_ENABLE) {
        reset(base, SW_RESET_DAT);
        return false;
    }

    // If SDMA didn't handle it, write data via PIO
    let ps = reg_r(base, PRESENT_STATE);
    if ps & PS_BUFFER_WRITE_ENABLE != 0 {
        for i in (0..512).step_by(4) {
            let word = (buf[i] as u32)
                | ((buf[i + 1] as u32) << 8)
                | ((buf[i + 2] as u32) << 16)
                | ((buf[i + 3] as u32) << 24);
            reg_w(base, BUFFER_DATA, word);
        }
    }

    // Wait for transfer complete
    wait_transfer_complete(base)
}

/// Read `n_sectors` consecutive 512-byte sectors starting at `lba` into `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of writable memory.
pub unsafe fn read_multi(lba: u64, n_sectors: u32, buf: *mut u8) -> bool {
    let mut sector_buf = [0u8; SDHCI_SECTOR_SIZE];
    for i in 0u32..n_sectors {
        if !read(lba + i as u64, &mut sector_buf) {
            return false;
        }
        core::ptr::copy_nonoverlapping(
            sector_buf.as_ptr(),
            buf.add((i as usize) * SDHCI_SECTOR_SIZE),
            SDHCI_SECTOR_SIZE,
        );
    }
    true
}

/// Write `n_sectors` consecutive 512-byte sectors starting at `lba` from `buf`.
/// `buf` must point to at least `n_sectors * 512` bytes of readable memory.
pub unsafe fn write_multi(lba: u64, n_sectors: u32, buf: *const u8) -> bool {
    let mut sector_buf = [0u8; SDHCI_SECTOR_SIZE];
    for i in 0u32..n_sectors {
        core::ptr::copy_nonoverlapping(
            buf.add((i as usize) * SDHCI_SECTOR_SIZE),
            sector_buf.as_mut_ptr(),
            SDHCI_SECTOR_SIZE,
        );
        if !write(lba + i as u64, &sector_buf) {
            return false;
        }
    }
    true
}

/// PLIC interrupt handler for SDHCI.
pub unsafe fn irq_handler() {
    let p = &raw const G_SDHCI;
    if !(*p).initialized {
        return;
    }
    let base = (*p).base;
    // Read and clear interrupt status
    let istat = reg_r(base, NORMAL_INT_STATUS);
    reg_w(base, NORMAL_INT_STATUS, istat);
    let err = reg_r(base, ERROR_INT_STATUS);
    if err != 0 {
        reg_w(base, ERROR_INT_STATUS, err);
    }
}

/// Return whether the SDHCI controller has been initialized.
pub fn is_initialized() -> bool {
    unsafe { (*(&raw const G_SDHCI)).initialized }
}

/// Return the base address of the initialized SDHCI controller.
pub fn base_addr() -> usize {
    unsafe { (*(&raw const G_SDHCI)).base }
}
