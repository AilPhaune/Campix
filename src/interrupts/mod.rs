use core::arch::asm;

pub mod handlers;
pub mod idt;
pub mod pic;

pub fn init() {
    pic::pic_remap(0x20, 0x28);
    for i in 0..16 {
        pic::pic_mask(i);
    }
    pic::pic_unmask(1);
    idt::init_interrupts();
    unsafe {
        asm!("sti");
    }
}
