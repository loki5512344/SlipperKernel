OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    .text : ALIGN(4) { KEEP(*(.text.start)) *(.text .text.*) }
    .rodata : ALIGN(8) { *(.rodata .srodata .rodata.* .srodata.*) }
    .data : ALIGN(8) { *(.data .sdata .data.* .sdata.*) }
    . = ALIGN(8); __bss_start = .;
    .bss : ALIGN(8) { *(.bss .sbss .bss.* .sbss.* COMMON) }
    . = ALIGN(8); __bss_end = .;
    /DISCARD/ : { *(.note .note.*) *(.comment) *(.eh_frame .eh_frame.*) *(.riscv.attributes) }
}
