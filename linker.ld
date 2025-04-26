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
    .data ALIGN(4K) : {
        *(.data*)
        *(.ldata*)
    }
    .bss ALIGN(4K) (NOLOAD) : {
        *(.bss*)
        *(.lbss*)
    }

    . = ALIGN(4K);

    /DISCARD/ : {
        *(.eh_frame*)
        *(.eh_frame_hdr*)
        *(.comment*)
    }
}