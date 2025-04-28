use core::arch::asm;

pub mod handlers;
pub mod idt;
pub mod pic;
pub mod pit;

pub fn init() {
    pic::pic_remap(0x20, 0x28);
    pit::init_pit(u16::MAX);

    idt::init_interrupts();

    pic::pic_unmask(0);
    pic::pic_unmask(1);

    unsafe {
        asm!("sti");
    }
}
