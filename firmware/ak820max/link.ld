ENTRY(aj_usb)
MEMORY { ROM (rx) : ORIGIN = 0x20000, LENGTH = 0x4000 }
SECTIONS {
 .text : { *(.text*) *(.rodata*) } > ROM
 /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) *(.note*) }
 .forbidden : { *(.data*) *(.bss*) *(COMMON) } > ROM
 ASSERT(SIZEOF(.forbidden) == 0, "Use only the explicitly reserved state")
}
