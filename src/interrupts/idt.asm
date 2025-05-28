BITS 64

%macro save_regs 0
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push rsp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
%endmacro

%macro restore_regs 0
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsp
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
%endmacro

%macro align_stack 0
    ; Save old rsp
    mov rax, rsp
    ; Align down
    and rsp, ~0xF
    ; Push old rsp
    push rax
    ; Push dummy value to align stack
    push QWORD 0
%endmacro

%macro pop_stack 0
    pop rax
    pop rsp
%endmacro

%macro isr_err_stub 1
isr_stub_%+%1:
    save_regs
    
    ; Args
    mov rdi, %1
    lea rsi, [rsp + 16*8]

    align_stack
    call idt_exception_handler
    pop_stack

    restore_regs
    iretq
%endmacro

%macro isr_no_err_stub 1
isr_stub_%+%1:
    save_regs
    mov rdi, %1
    lea rsi, [rsp + 16*8]
    align_stack
    call idt_exception_handler
    pop_stack
    restore_regs
    iretq
%endmacro

%macro isr_pic_stub 1
isr_stub_%+%1:
    save_regs
    mov rdi, %1
    lea rsi, [rsp + 16*8]
    align_stack
    call idt_irq_handler
    pop_stack
    restore_regs
    iretq
%endmacro

extern idt_exception_handler
extern idt_irq_handler
extern idt_software_interrupt_handler

isr_no_err_stub 0
isr_no_err_stub 1
isr_no_err_stub 2
isr_no_err_stub 3
isr_no_err_stub 4
isr_no_err_stub 5
isr_no_err_stub 6
isr_no_err_stub 7
isr_err_stub    8
isr_no_err_stub 9
isr_err_stub    10
isr_err_stub    11
isr_err_stub    12
isr_err_stub    13
isr_err_stub    14
isr_no_err_stub 15
isr_no_err_stub 16
isr_err_stub    17
isr_no_err_stub 18
isr_no_err_stub 19
isr_no_err_stub 20
isr_no_err_stub 21
isr_no_err_stub 22
isr_no_err_stub 23
isr_no_err_stub 24
isr_no_err_stub 25
isr_no_err_stub 26
isr_no_err_stub 27
isr_no_err_stub 28
isr_no_err_stub 29
isr_err_stub    30
isr_no_err_stub 31

isr_pic_stub    32
isr_pic_stub    33
isr_pic_stub    34
isr_pic_stub    35
isr_pic_stub    36
isr_pic_stub    37
isr_pic_stub    38
isr_pic_stub    39
isr_pic_stub    40
isr_pic_stub    41
isr_pic_stub    42
isr_pic_stub    43
isr_pic_stub    44
isr_pic_stub    45
isr_pic_stub    46
isr_pic_stub    47

%assign i 48
%rep    208
isr_stub_%+i:
    save_regs
    mov rdi, i
    lea rsi, [rsp + 16*8]
    align_stack
    call idt_software_interrupt_handler
    pop_stack
    restore_regs
    iretq
%assign i i+1 
%endrep

global isr_stub_table
isr_stub_table:
%assign i 0 
%rep    256
    dq isr_stub_%+i
%assign i i+1 
%endrep