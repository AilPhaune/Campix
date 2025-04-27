use crate::io::{inb, outb};

#[no_mangle]
pub fn write_char(character: u8) {
    // BOCHS
    outb(0xE9, character);

    // QEMU
    while inb(0x379) & 0b01000000 == 0 {}
    outb(0x378, character);
    outb(0x37A, inb(0x37A) | 1);
    while inb(0x379) & 0b00100000 != 0 {}
    outb(0x37A, inb(0x37A) & 0b11111110);
}

pub struct E9Writer {}

impl core::fmt::Write for E9Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            if c == '\n' {
                write_char(b'\r');
            }
            write_char(c as u8);
        }
        Ok(())
    }

    fn write_char(&mut self, c: char) -> core::fmt::Result {
        write_char(c as u8);
        Ok(())
    }

    fn write_fmt(&mut self, fmt: core::fmt::Arguments) -> core::fmt::Result {
        core::fmt::write(self, fmt)?;
        Ok(())
    }
}

#[macro_export]
macro_rules! printf {
    ($fmt: expr) => {{
        use core::fmt::Write;
        let mut writer = $crate::e9::E9Writer {};
        write!(writer, $fmt).unwrap();
    }};
    ($fmt: expr, $( $arg: expr ),*) => {{
        use core::fmt::Write;
        let mut writer = $crate::e9::E9Writer {};
        write!(writer, $fmt, $( $arg ),*).unwrap();
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::printf!("\n");
    }};
    ($fmt: expr) => {{
        use core::fmt::Write;
        let mut writer = $crate::e9::E9Writer {};
        write!(writer, $fmt).unwrap();
        write!(writer, "\n").unwrap();
    }};
    ($fmt: expr, $( $arg: expr ),*) => {{
        use core::fmt::Write;
        let mut writer = $crate::e9::E9Writer {};
        write!(writer, $fmt, $( $arg ),*).unwrap();
        write!(writer, "\n").unwrap();
    }};
}
