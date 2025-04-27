use core::arch::asm;

use crate::{gdt::KERNEL_CODE_SELECTOR, interrupts::pic::pic_send_eoi, println};

use super::handlers;

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

    fn set_handler(&mut self, handler: extern "C" fn(), selector: u16, ist: u8, attributes: u8) {
        let addr = handler as usize as u64;
        self.isr_low = (addr & 0xFFFF) as u16;
        self.kernerl_cs = selector;
        self.ist = ist;
        self.flags = attributes;
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

fn unhandled_interrupt(int: u64) {
    println!("Unhandled interrupt {:#02x} !", int);
    #[allow(clippy::empty_loop)]
    loop {}
}

static mut HANDLERS: [fn(u64); 256] = [unhandled_interrupt; 256];

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
    static isr_stub_table: [extern "C" fn(); 48];
}

#[no_mangle]
pub extern "C" fn idt_exception_handler(interrupt_num: u64) {
    unsafe {
        HANDLERS[interrupt_num as usize](interrupt_num);
    }
}

#[no_mangle]
pub extern "C" fn idt_irq_handler(interrupt_num: u64) {
    unsafe {
        HANDLERS[interrupt_num as usize](interrupt_num);
    }

    pic_send_eoi(interrupt_num as u8 - 32);
}

pub fn init_interrupts() {
    unsafe {
        for (i, f) in isr_stub_table.iter().enumerate() {
            IDT.entries[i].set_handler(*f, KERNEL_CODE_SELECTOR as u16, 0, 0x8E);
        }

        HANDLERS[0x21] = handlers::irq1_keyboard::handler;

        #[allow(static_mut_refs)]
        load_idt(&IDT);
    }
}
