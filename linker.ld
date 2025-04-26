ENTRY(_start)

SECTIONS {
    . = 0xFFFF800000000000;

    .text ALIGN(4K) : {
        *(.text._start)
        *(.text*)
        *(.ltext*)
    }
    .rodata ALIGN(4K) : { 
        *(.rodata*)
        *(.lrodata*)
    }
    .data ALIGN(4K) : { *(.data*) }
    .bss ALIGN(4K) (NOLOAD) : { *(.bss*) }

    . = ALIGN(4K);
}