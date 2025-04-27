use core::arch::asm;

pub fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value);
    }
}

pub fn outw(port: u16, value: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value);
    }
}

pub fn outl(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value);
    }
}

pub fn inb(port: u16) -> u8 {
    let result: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") result);
    }
    result
}

pub fn inw(port: u16) -> u16 {
    let result: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") result);
    }
    result
}

pub fn inl(port: u16) -> u32 {
    let result: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") result);
    }
    result
}

const UNUSED_PORT: u16 = 0x80;
pub fn iowait() {
    for _ in 0..1000 {
        outb(UNUSED_PORT, 0);
    }
}
