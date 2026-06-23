OUTPUT_ARCH(riscv)
ENTRY(_start)
KERNEL_BASE = 0x80200000;
PHDRS { text PT_LOAD FLAGS(5); rodata PT_LOAD FLAGS(4); data PT_LOAD FLAGS(6); bss PT_LOAD FLAGS(6); }
SECTIONS {
    . = KERNEL_BASE;
    .text : ALIGN(4) { KEEP(*(.text.boot)) KEEP(*(.text.trap)) *(.text .text.*) } :text
    .rodata : ALIGN(8) { *(.rodata .srodata .rodata.* .srodata.*) } :rodata
    .data : ALIGN(8) { *(.data .sdata .data.* .sdata.*) } :data
    . = ALIGN(8); __bss_start = .;
    .bss : ALIGN(8) { *(.bss .sbss .bss.* .sbss.* COMMON) } :bss
    . = ALIGN(8); __bss_end = .;
    . = ALIGN(16); __stack_bottom = .;
    . = . + 0x10000;
    . = ALIGN(4096); __stack_top = .;
    __kernel_end = .;
    /DISCARD/ : { *(.note .note.*) *(.comment) *(.eh_frame .eh_frame.*) *(.riscv.attributes) }
}
