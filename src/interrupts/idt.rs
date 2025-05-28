use core::arch::asm;

use crate::{gdt::KERNEL_CODE_SELECTOR, interrupts::pic::pic_send_eoi, println};

use super::handlers;

pub const IDT_PRESENT: u8 = 1 << 7;
pub const IDT_DPL0: u8 = 0 << 5;
pub const IDT_DPL1: u8 = 1 << 5;
pub const IDT_DPL2: u8 = 2 << 5;
pub const IDT_DPL3: u8 = 3 << 5;
pub const IDT_STORAGE_SEGMENT: u8 = 0 << 4;

pub const IDT_TYPE_TASK_GATE: u8 = 0x5;
pub const IDT_TYPE_16BIT_INT_GATE: u8 = 0x6;
pub const IDT_TYPE_16BIT_TRAP_GATE: u8 = 0x7;
pub const IDT_TYPE_32BIT_INT_GATE: u8 = 0xE;
pub const IDT_TYPE_32BIT_TRAP_GATE: u8 = 0xF;
pub const IDT_TYPE_64BIT_INT_GATE: u8 = IDT_TYPE_32BIT_INT_GATE;

pub const KERNEL_INT_FLAGS: u8 =
    IDT_PRESENT | IDT_DPL0 | IDT_STORAGE_SEGMENT | IDT_TYPE_64BIT_INT_GATE;
pub const USER_INT_FLAGS: u8 =
    IDT_PRESENT | IDT_DPL3 | IDT_STORAGE_SEGMENT | IDT_TYPE_64BIT_INT_GATE;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry64 {
    isr_low: u16,
    kernerl_cs: u16,
    ist: u8,
    flags: u8,
    isr_mid: u16,
    isr_high: u32,
    reserved: u32,
}

impl IdtEntry64 {
    const fn missing() -> Self {
        Self {
            isr_low: 0,
            kernerl_cs: 0,
            ist: 0,
            flags: 0,
            isr_mid: 0,
            isr_high: 0,
            reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: extern "C" fn(), selector: u16, ist: u8, flags: u8) {
        let addr = handler as usize as u64;
        self.isr_low = (addr & 0xFFFF) as u16;
        self.kernerl_cs = selector;
        self.ist = ist;
        self.flags = flags;
        self.isr_mid = ((addr >> 16) & 0xFFFF) as u16;
        self.isr_high = (addr >> 32) as u32;
        self.reserved = 0;
    }
}

#[repr(C, align(16))]
struct Idt {
    entries: [IdtEntry64; 256],
}

static mut IDT: Idt = Idt {
    entries: [IdtEntry64::missing(); 256],
};

fn unhandled_interrupt(int: u64, _rsp: u64) {
    panic!("Unhandled interrupt {:#02x} !", int);
}

static mut HANDLERS: [fn(u64, u64); 256] = [unhandled_interrupt; 256];

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

unsafe fn load_idt(idt: &Idt) {
    let descriptor = IdtDescriptor {
        limit: (core::mem::size_of::<Idt>() - 1) as u16,
        base: idt as *const _ as u64,
    };
    asm!(
        "lidt [{}]",
        in(reg) &descriptor,
        options(readonly, nostack, preserves_flags)
    );
}

extern "C" {
    static isr_stub_table: [extern "C" fn(); 256];
}

#[no_mangle]
pub extern "C" fn idt_exception_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        HANDLERS[interrupt_num as usize](interrupt_num, rsp);
    }
}

#[no_mangle]
pub extern "C" fn idt_irq_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        HANDLERS[interrupt_num as usize](interrupt_num, rsp);
    }

    pic_send_eoi(interrupt_num as u8 - 32);
}

#[no_mangle]
pub extern "C" fn idt_software_interrupt_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        HANDLERS[interrupt_num as usize](interrupt_num, rsp);
    }
}

pub fn init_interrupts() {
    unsafe {
        for (i, f) in isr_stub_table.iter().enumerate() {
            IDT.entries[i].set_handler(*f, KERNEL_CODE_SELECTOR as u16, 0, KERNEL_INT_FLAGS);
        }
        IDT.entries[0x80].flags = USER_INT_FLAGS;

        HANDLERS[0x20] = handlers::irq0_timer::handler;
        HANDLERS[0x21] = handlers::irq1_keyboard::handler;

        HANDLERS[0x06] = inv_opcode;
        HANDLERS[0x0E] = pgfault;

        HANDLERS[0x80] = x80_handle;

        #[allow(static_mut_refs)]
        load_idt(&IDT);
    }
}

fn inv_opcode(_ist: u64, rsp: u64) {
    unsafe {
        println!("Invalid opcode");
        println!("rsp = {:#x}", rsp);

        for i in -20..20 {
            println!("[rsp + 8*{}] = {:#x}", i, *(rsp as *const u64).offset(i));
        }

        let rip = *(rsp as *const u64);
        println!("rip = {:#x}", rip);

        panic!("int 0x06");
    }
}

fn pgfault(_ist: u64, _rsp: u64) {
    panic!("Page fault");
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Registers {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rsp: u64,
    rbp: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    rcx: u64,
    rbx: u64,
    rax: u64,
}

fn x80_handle(_ist: u64, rsp: u64) {
    unsafe {
        println!("x80 handle");

        println!(
            "regs = {:#?}",
            ((rsp - size_of::<Registers>() as u64) as *const Registers).read_unaligned()
        );

        println!("rsp = {:#x}", rsp);

        for i in -20..20 {
            println!("[rsp + 8*{}] = {:#x}", i, *(rsp as *const u64).offset(i));
        }

        let rip = *(rsp as *const u64);
        println!("rip = {:#x}", rip);
    }
}
