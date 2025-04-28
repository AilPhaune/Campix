use crate::io::{inb, iowait, outb};

pub const PIC1: u16 = 0x20;
pub const PIC2: u16 = 0xA0;

pub const PIC1_COMMAND: u16 = PIC1;
pub const PIC1_DATA: u16 = PIC1 + 1;

pub const PIC2_COMMAND: u16 = PIC2;
pub const PIC2_DATA: u16 = PIC2 + 1;

pub const PIC_EOI: u8 = 0x20;

pub fn pic_send_eoi(int: u8) {
    if int >= 8 {
        outb(PIC2_COMMAND, PIC_EOI);
    }

    outb(PIC1_COMMAND, PIC_EOI);
}

pub const ICW1_ICW4: u8 = 0x01;
pub const ICW1_SINGLE: u8 = 0x02;
pub const ICW1_INTERVAL4: u8 = 0x04;
pub const ICW1_LEVEL: u8 = 0x08;
pub const ICW1_INIT: u8 = 0x10;

pub const ICW4_8086: u8 = 0x01;
pub const ICW4_AUTO: u8 = 0x02;
pub const ICW4_BUF_SLAVE: u8 = 0x08;
pub const ICW4_BUF_MASTER: u8 = 0x0C;
pub const ICW4_SFNM: u8 = 0x10;

pub fn pic_remap(offset1: usize, offset2: usize) {
    outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
    iowait();

    outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
    iowait();

    outb(PIC1_DATA, offset1 as u8);
    iowait();

    outb(PIC2_DATA, offset2 as u8);
    iowait();

    outb(PIC1_DATA, 4);
    iowait();

    outb(PIC2_DATA, 2);
    iowait();

    outb(PIC1_DATA, ICW4_8086);
    iowait();

    outb(PIC2_DATA, ICW4_8086);
    iowait();

    outb(PIC1_DATA, 0xFF);
    iowait();
    outb(PIC2_DATA, 0xFF);
    iowait();
}

pub fn pic_disable() {
    outb(PIC1_DATA, 0xFF);
    iowait();
    outb(PIC2_DATA, 0xFF);
    iowait();
}

pub fn pic_mask(mut irq: u8) {
    let port;

    if irq < 8 {
        port = PIC1_DATA;
    } else {
        port = PIC2_DATA;
        irq -= 8;
    }

    let value = inb(port) | (1 << irq);
    outb(port, value);
}

pub fn pic_unmask(mut irq: u8) {
    let port;

    if irq < 8 {
        port = PIC1_DATA;
    } else {
        port = PIC2_DATA;
        irq -= 8;
    }

    let value = inb(port) & !(1 << irq);
    outb(port, value);
}
