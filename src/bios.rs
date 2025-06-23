use crate::paging::DIRECT_MAPPING_OFFSET;

/// https://wiki.osdev.org/Memory_Map_(x86)#BIOS_Data_Area_(BDA)
#[repr(C, packed)]
#[derive(Debug)]
pub struct BiosDataArea {
    pub com_serial_base_io: [u16; 4],
    pub lpt_parallel_base_io: [u16; 3],
    pub ebda_base_addr: u16,
    pub detected_hardware: u16,
    pub _reserved: u8,
    pub unusable_memory_kb: u16,
    pub _reserved2: u16,
    pub keyboard_state: u16,
    pub _reserved3: [u8; 5],
    pub keyboard_buffer: [u8; 32],
    pub _reserved4: [u8; 11],
    pub display_mode: u8,
    pub text_mode_columns: u16,
    pub _reserved5: [u8; 23],
    pub video_base_io: [u8; 2],
    pub _reserved6: [u8; 7],
    pub irq0_count: u16,
    pub _reserved7: [u8; 7],
    pub hard_disk_count: u8,
    pub _reserved8: [u8; 10],
    pub keyboard_buffer_start: u16,
    pub keyboard_buffer_end: u16,
    pub _reserved9: [u8; 19],
    pub keyboard_led_shift_state: u8,
}

pub const BDA: *mut BiosDataArea = (0x400 + DIRECT_MAPPING_OFFSET) as *mut BiosDataArea;

static mut BDA_DATA: Option<BiosDataArea> = None;

#[allow(static_mut_refs)]
pub fn get_bda() -> &'static BiosDataArea {
    unsafe {
        match &BDA_DATA {
            Some(bda) => bda,
            None => {
                BDA_DATA = Some(core::ptr::read_unaligned(BDA));
                BDA_DATA.as_ref().unwrap()
            }
        }
    }
}
