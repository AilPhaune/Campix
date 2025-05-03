use crate::{obsiboot::ObsiBootKernelParameters, paging::DIRECT_MAPPING_OFFSET};

#[repr(C, packed)]
pub struct VbeInfoBlock {
    pub signature: [u8; 4],
    pub version: u16,
    pub oem_string_ptr: [u16; 2],
    pub capabilities: [u8; 4],
    pub video_mode_ptr: [u16; 2],
    pub total_memory: u16,
    pub reserved: [u8; 492],
}

#[repr(C, packed)]
#[derive(Clone, Debug)]
pub struct VesaModeInfoStructure {
    pub attributes: u16,
    pub window_a: u8,
    pub window_b: u8,
    pub granularity: u16,
    pub window_size: u16,
    pub segment_a: u16,
    pub segment_b: u16,
    pub win_func_ptr: u32,
    pub pitch: u16,
    pub width: u16,
    pub height: u16,
    pub w_char: u8,
    pub y_char: u8,
    pub planes: u8,
    pub bpp: u8,
    pub banks: u8,
    pub memory_model: u8,
    pub bank_size: u8,
    pub image_pages: u8,
    pub reserved0: u8,
    pub red_mask: u8,
    pub red_position: u8,
    pub green_mask: u8,
    pub green_position: u8,
    pub blue_mask: u8,
    pub blue_position: u8,
    pub reserved_mask: u8,
    pub reserved_position: u8,
    pub direct_color_attributes: u8,
    pub framebuffer: u32,
    pub offscreen_mem_off: u32,
    pub offscreen_mem_size: u16,
    pub reserved1: [u8; 206],
}

static mut CURRENT_MODE: Option<VesaModeInfoStructure> = None;

pub struct VesaModeIterator {
    video_mode_ptr: *const u16,
    modes_info_ptr: *const VesaModeInfoStructure,
    modes_count: usize,
    index: usize,
}

impl Iterator for VesaModeIterator {
    type Item = (u16, VesaModeInfoStructure);

    fn next(&mut self) -> Option<Self::Item> {
        let mode = unsafe { self.video_mode_ptr.add(self.index).read_unaligned() };
        if self.index >= self.modes_count || mode == 0xFFFF {
            None
        } else {
            let mode_info = unsafe { self.modes_info_ptr.add(self.index).read_unaligned() };
            self.index += 1;
            Some((mode, mode_info))
        }
    }
}

pub fn iter_modes(obsiboot: &ObsiBootKernelParameters) -> VesaModeIterator {
    let vbe_info_block = unsafe {
        ((obsiboot.vbe_info_block_ptr as u64 + DIRECT_MAPPING_OFFSET) as *const VbeInfoBlock)
            .read_unaligned()
    };

    let video_mode_ptr = (vbe_info_block.video_mode_ptr[1] as u64 * 16
        + vbe_info_block.video_mode_ptr[0] as u64
        + DIRECT_MAPPING_OFFSET) as *const u16;

    let modes_info_ptr = (obsiboot.vbe_modes_info_ptr as u64 + DIRECT_MAPPING_OFFSET)
        as *const VesaModeInfoStructure;

    VesaModeIterator {
        video_mode_ptr,
        modes_info_ptr,
        modes_count: obsiboot.vbe_mode_info_block_entry_count as usize,
        index: 0,
    }
}

pub fn parse_current_mode(obsiboot: &ObsiBootKernelParameters) {
    let vbe_info_block = unsafe {
        ((obsiboot.vbe_info_block_ptr as u64 + DIRECT_MAPPING_OFFSET) as *const VbeInfoBlock)
            .read_unaligned()
    };

    let video_mode_ptr = (vbe_info_block.video_mode_ptr[1] as u64 * 16
        + vbe_info_block.video_mode_ptr[0] as u64
        + DIRECT_MAPPING_OFFSET) as *const u16;

    let selected_mode = obsiboot.vbe_selected_mode;

    let mut i = 0;
    let selected_mode_idx = loop {
        let mode = unsafe { video_mode_ptr.add(i).read_unaligned() };
        if mode == 0xFFFF {
            panic!("Vesa mode {} not found !", selected_mode);
        }

        if mode as u32 == selected_mode {
            break i;
        }
        i += 1;
    };

    let modes_info_ptr = (obsiboot.vbe_modes_info_ptr as u64 + DIRECT_MAPPING_OFFSET)
        as *const VesaModeInfoStructure;
    if i >= obsiboot.vbe_mode_info_block_entry_count as usize {
        panic!(
            "Vesa mode {} not found in modes info block !",
            selected_mode
        );
    }

    unsafe {
        CURRENT_MODE = Some(
            modes_info_ptr
                .add(selected_mode_idx as usize)
                .read_unaligned(),
        );
    }
}

pub fn get_mode_info() -> VesaModeInfoStructure {
    #[allow(static_mut_refs)]
    unsafe {
        CURRENT_MODE.clone().unwrap()
    }
}
