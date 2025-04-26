target remote :1234
set architecture i8086
symbol-file kbuild/kernel.debug
break _start
layout asm
set disassembly-flavor intel
continue
