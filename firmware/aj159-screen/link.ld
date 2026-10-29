ENTRY(screen_usb_hook)
SECTIONS {
    . = 0x0105a000;
    .text : { *(.text*) *(.rodata*) }
    .data : { *(.data*) }
    .bss (NOLOAD) : { *(.bss*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
    ASSERT(SIZEOF(.data) == 0, "Extension must not require initialized globals")
    ASSERT(SIZEOF(.bss) == 0, "Extension uses a bounded, explicit RAM reservation")
    ASSERT(. <= 0x01060000, "Extension exceeds the existing application erase range")
}
