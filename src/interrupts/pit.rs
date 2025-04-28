use crate::io::outb;

pub const PIT_COMMAND_PORT: u16 = 0x43;
pub const PIT_CHANNEL0_DATA_PORT: u16 = 0x40;

pub fn init_pit(frequency_divider: u16) {
    outb(PIT_COMMAND_PORT, 0x36);
    outb(PIT_CHANNEL0_DATA_PORT, (frequency_divider & 0xFF) as u8);
    outb(
        PIT_CHANNEL0_DATA_PORT,
        ((frequency_divider >> 8) & 0xFF) as u8,
    );
}
