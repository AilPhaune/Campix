use core::{alloc::Layout, panic};

use alloc::{alloc::alloc_zeroed, boxed::Box, collections::BTreeSet, sync::Arc};
use spin::RwLock;

use crate::{
    paging, permissions,
    vesa::{get_mode_info, VesaModeInfoStructure},
};

use super::{
    fs::virt::devfs::{fseek_helper, DevFs, DevFsDriver, DevFsHook, DevFsHookKind},
    pci::PciDevice,
    vfs::{
        arcrwb_new_from_box, Arcrwb, CharacterDevice, FileStat, FileSystem, VfsError, VfsFile,
        VfsFileKind, FLAG_SYSTEM, FLAG_VIRTUAL_CHARACTER_DEVICE, OPEN_MODE_APPEND,
        OPEN_MODE_BINARY, OPEN_MODE_READ, OPEN_MODE_WRITE,
    },
};

#[derive(Debug)]
pub struct VgaCharDevice {
    mode_info: VesaModeInfoStructure,

    width: u64,
    height: u64,
    bpp: u64,
    pixel_count: u64,
    size: u64,

    framebuffer: RwLock<usize>,
    bytes_per_line: u64,
    frame_buffer_is_xrgb: bool,

    double_buffer_size: u64,
    double_buffer: RwLock<usize>,
}

impl VgaCharDevice {
    pub fn get_width(&self) -> u64 {
        self.width
    }

    pub fn get_height(&self) -> u64 {
        self.height
    }

    pub fn get_bpp(&self) -> u64 {
        self.bpp
    }

    pub fn get_pixel_count(&self) -> u64 {
        self.pixel_count
    }

    pub fn get_bytes_per_line(&self) -> u64 {
        self.bytes_per_line
    }

    pub fn get_frame_buffer_is_xrgb(&self) -> bool {
        self.frame_buffer_is_xrgb
    }

    pub fn get_double_buffer_size(&self) -> u64 {
        self.double_buffer_size
    }

    unsafe fn ensure_framebuffer_mapped(fb: u64, size: u64) {
        paging::map_memory(
            fb,
            size,
            paging::DIRECT_MAPPING_OFFSET,
            paging::PAGE_ACCESSED | paging::PAGE_RW,
        );
    }

    pub fn new(mode_info: VesaModeInfoStructure) -> Self {
        let width = mode_info.width as u64;
        let height = mode_info.height as u64;
        let bpp = mode_info.bpp as u64;
        if bpp != 32 && bpp != 24 {
            panic!("Unsupported bpp {} for VGA driver", bpp);
        }
        let bytes_per_pixel = bpp / 8;
        let pixel_count = width * height;
        let size = pixel_count * bytes_per_pixel;

        unsafe {
            Self::ensure_framebuffer_mapped(
                mode_info.framebuffer as u64,
                mode_info.pitch as u64 * height,
            );
        }

        let framebuffer =
            RwLock::new((mode_info.framebuffer as u64 + paging::DIRECT_MAPPING_OFFSET) as usize);
        let bytes_per_line = mode_info.pitch as u64;

        let double_buffer_size = width * height * 4;
        let layout = Layout::from_size_align(double_buffer_size as usize, 4).unwrap();
        let double_buffer = RwLock::new(unsafe { alloc_zeroed(layout) as usize });

        let frame_buffer_is_xrgb = bpp == 32
            && mode_info.red_position == 16
            && mode_info.green_position == 8
            && mode_info.blue_position == 0;

        Self {
            mode_info,
            width,
            height,
            bpp,
            pixel_count,
            size,
            framebuffer,
            bytes_per_line,
            frame_buffer_is_xrgb,
            double_buffer_size,
            double_buffer,
        }
    }

    /// Reads a pixel from the doubled framebuffer
    #[inline(always)]
    pub fn read_pixel_at_offset(&self, offset: u64) -> u32 {
        if offset >= self.size {
            return 0;
        }
        let guard = self.double_buffer.read();
        unsafe { *(*guard as *const u32).add(offset as usize) }
    }

    /// Reads a pixel from the doubled framebuffer
    #[inline(always)]
    pub fn read_pixel(&mut self, x: u64, y: u64) -> u32 {
        self.read_pixel_at_offset(x + y * self.width)
    }

    /// Writes a pixel to the doubled framebuffer
    #[inline(always)]
    pub fn write_pixel_at_offset(&mut self, offset: u64, color: u32) {
        if offset < self.pixel_count {
            unsafe {
                let guard = self.double_buffer.write();
                *(*guard as *mut u32).add(offset as usize) = color;
            }
        }
    }

    /// Writes a pixel to the doubled framebuffer
    #[inline(always)]
    pub fn write_pixel(&mut self, x: u64, y: u64, color: u32) {
        self.write_pixel_at_offset(x + y * self.width, color);
    }

    /// Swaps the buffers
    #[inline(always)]
    pub fn swap_buffers(&mut self) {
        macro_rules! convert_xrgb_to_custom {
            ($xrgb:expr, $red_pos:expr, $green_pos:expr, $blue_pos:expr) => {{
                let r = ($xrgb >> 16) & 0xFF;
                let g = ($xrgb >> 8) & 0xFF;
                let b = $xrgb & 0xFF;
                ((r << $red_pos) | (g << $green_pos) | (b << $blue_pos)) as u32
            }};
        }

        let guard = self.double_buffer.read();
        let fguard = self.framebuffer.read();

        let backbuffer = *guard as *mut u8;
        let framebuffer = *fguard as *mut u8;

        let pitch = self.bytes_per_line;
        let width = self.width;
        let height = self.height;

        if self.frame_buffer_is_xrgb {
            if pitch == width * 4 {
                unsafe {
                    core::ptr::copy_nonoverlapping(backbuffer, framebuffer, self.size as usize);
                }
            } else {
                for y in 0..height {
                    unsafe {
                        let src = backbuffer.add((y * width * 4) as usize);
                        let dst = framebuffer.add((y * pitch) as usize);
                        core::ptr::copy_nonoverlapping(src, dst, width as usize * 4);
                    }
                }
            }
        } else if self.bpp == 32 {
            let r_pos = self.mode_info.red_position;
            let g_pos = self.mode_info.green_position;
            let b_pos = self.mode_info.blue_position;

            for y in 0..height {
                for x in 0..width {
                    let offset = y * width + x;
                    let pixel = unsafe { *(backbuffer.add(offset as usize * 4) as *const u32) };
                    let converted = convert_xrgb_to_custom!(pixel, r_pos, g_pos, b_pos);

                    unsafe {
                        let dst = framebuffer.add((y * pitch + x * 4) as usize) as *mut u32;
                        *dst = converted;
                    }
                }
            }
        } else {
            // bpp = 24
            let r_pos = self.mode_info.red_position as u32;
            let g_pos = self.mode_info.green_position as u32;
            let b_pos = self.mode_info.blue_position as u32;

            let is_rgb = r_pos == 16 && g_pos == 8 && b_pos == 0;

            for y in 0..height {
                for x in 0..width {
                    let offset = y * width + x;
                    let pixel = unsafe { *(backbuffer.add(offset as usize * 4) as *const u32) };

                    let (r, g, b) = (
                        ((pixel >> 16) & 0xFF) as u8,
                        ((pixel >> 8) & 0xFF) as u8,
                        (pixel & 0xFF) as u8,
                    );

                    let dst_offset = (y * pitch + x * 3) as usize;
                    unsafe {
                        let dst = framebuffer.add(dst_offset);
                        if is_rgb {
                            *dst = b;
                            *dst.add(1) = g;
                            *dst.add(2) = r;
                        } else {
                            // Reorder components based on positions
                            let mut rgb = [0u8; 3];
                            rgb[(r_pos / 8) as usize] = r;
                            rgb[(g_pos / 8) as usize] = g;
                            rgb[(b_pos / 8) as usize] = b;

                            *dst = rgb[0];
                            *dst.add(1) = rgb[1];
                            *dst.add(2) = rgb[2];
                        }
                    }
                }
            }
        }
    }
}

impl CharacterDevice for VgaCharDevice {
    fn get_generation(&self) -> u64 {
        0
    }

    fn get_size(&self) -> u64 {
        self.double_buffer_size
    }

    fn read_chars(&self, offset: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if offset >= self.double_buffer_size {
            return Err(VfsError::OutOfBounds);
        }
        let max_read = (self.size - offset).min(buf.len() as u64);
        let guard = self.double_buffer.read();
        unsafe {
            core::ptr::copy_nonoverlapping(
                (*guard as *const u8).add(offset as usize),
                buf.as_mut_ptr(),
                max_read as usize,
            );
        }
        Ok(max_read)
    }

    fn write_chars(&mut self, offset: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if offset >= self.double_buffer_size {
            return Err(VfsError::OutOfBounds);
        }
        let max_write = (self.size - offset).min(buf.len() as u64);
        let guard = self.double_buffer.write();
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf.as_ptr(),
                (*guard as *mut u8).add(offset as usize),
                max_write as usize,
            );
        }
        Ok(max_write)
    }

    fn flush(&mut self) -> Result<(), VfsError> {
        self.swap_buffers();

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct VgaFsFileHandle {
    mode: u64,
    device: Arcrwb<dyn CharacterDevice>,
    position: u64,
}

#[derive(Debug)]
pub struct VgaDriver {
    device: Arcrwb<dyn CharacterDevice>,
    size: u64,
    handles: BTreeSet<u64>,
}

impl VgaDriver {
    pub fn get_device(&self) -> Arcrwb<dyn CharacterDevice> {
        self.device.clone()
    }

    pub fn from_device(device: VgaCharDevice) -> Self {
        let size = device.get_size();
        Self {
            device: arcrwb_new_from_box(Box::new(device)),
            handles: BTreeSet::new(),
            size,
        }
    }
}

const VGA: u64 = u64::from_be_bytes([0, 0, 0, 0, 0, b'v', b'g', b'a']);

impl DevFsDriver for VgaDriver {
    fn handles_device(&self, _dev_fs: &mut DevFs, pci_device: &PciDevice) -> bool {
        pci_device.class == 0x03 && pci_device.subclass == 0x00
    }

    fn driver_id(&self) -> u64 {
        VGA
    }

    fn refresh_device_hooks(
        &mut self,
        dev_fs: &mut DevFs,
        pci_device: &PciDevice,
        _device_id: usize,
    ) -> Result<(), VfsError> {
        if !self.handles_device(dev_fs, pci_device) {
            return Err(VfsError::ActionNotAllowed);
        }
        let file = VfsFile::new(
            VfsFileKind::CharacterDevice {
                device: self.device.clone(),
            },
            alloc::vec!['v', 'g', 'a'],
            0,
            dev_fs.os_id(),
            dev_fs.os_id(),
        );
        dev_fs.replace_hook(
            "vga".chars().collect(),
            self.driver_id(),
            file,
            DevFsHookKind::Device,
            0,
        );
        Ok(())
    }

    fn fopen(
        &mut self,
        dev_fs: &mut DevFs,
        hook: Arc<DevFsHook>,
        mode: u64,
    ) -> Result<u64, VfsError> {
        if hook.file.name() != &['v', 'g', 'a'] {
            return Err(VfsError::PathNotFound);
        }

        let handle_data = VgaFsFileHandle {
            mode,
            device: self.device.clone(),
            position: 0,
        };

        if mode & OPEN_MODE_APPEND != 0 {
            return Err(VfsError::InvalidOpenMode);
        }
        if mode & OPEN_MODE_BINARY == 0 {
            return Err(VfsError::InvalidOpenMode);
        }

        let handle = dev_fs.alloc_file_handle::<VgaFsFileHandle>(handle_data, hook);
        self.handles.insert(handle);
        Ok(handle)
    }

    fn fclose(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::ActionNotAllowed);
        }
        self.handles.remove(&handle);
        dev_fs.dealloc_file_handle::<VgaFsFileHandle>(handle);
        Ok(())
    }

    fn fstat(&mut self, _dev_fs: &mut DevFs, handle: u64) -> Result<FileStat, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::ActionNotAllowed);
        }
        Ok(FileStat {
            size: self.size,
            is_directory: false,
            is_symlink: false,
            permissions: permissions!(Owner:Read, Owner:Write).to_u32(),
            owner_id: 0,
            group_id: 0,
            created_at: 0,
            modified_at: 0,
            flags: FLAG_VIRTUAL_CHARACTER_DEVICE | FLAG_SYSTEM,
        })
    }

    fn fseek(
        &mut self,
        dev_fs: &mut DevFs,
        handle: u64,
        position: super::vfs::SeekPosition,
    ) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<VgaFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };

        handle_data.position = fseek_helper(position, handle_data.position, self.size)
            .ok_or(VfsError::InvalidSeekPosition)?;

        Ok(handle_data.position)
    }

    fn fread(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<VgaFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };

        if handle_data.mode & OPEN_MODE_READ == 0 {
            return Err(VfsError::ActionNotAllowed);
        }

        let device = handle_data.device.read();
        let device = &**device;

        let bytes_read = device.read_chars(handle_data.position, buf)?;
        handle_data.position += bytes_read;
        Ok(bytes_read)
    }

    fn fwrite(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<VgaFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };

        if handle_data.mode & OPEN_MODE_WRITE == 0 {
            return Err(VfsError::ActionNotAllowed);
        }

        let mut device = handle_data.device.write();
        let device = &mut **device;

        let bytes_written = device.write_chars(handle_data.position, buf)?;
        handle_data.position += bytes_written;
        Ok(bytes_written)
    }

    fn fflush(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<VgaFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let mut device = handle_data.device.write();
        let device = &mut **device;

        device.flush()
    }

    fn fsync(&mut self, _dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        Ok(())
    }
}

static mut VGA_DRIVER: Option<Arcrwb<dyn DevFsDriver>> = None;

#[allow(static_mut_refs)]
pub fn get_vga_driver() -> Arcrwb<dyn DevFsDriver> {
    unsafe {
        if VGA_DRIVER.is_none() {
            let mode_info = get_mode_info();
            VGA_DRIVER = Some(arcrwb_new_from_box(Box::new(VgaDriver::from_device(
                VgaCharDevice::new(mode_info),
            ))));
        }
        VGA_DRIVER.clone().unwrap()
    }
}

pub fn init_vga(devfs: &mut DevFs) {
    let driver = get_vga_driver();
    devfs.register_driver(driver).unwrap();
}

pub fn use_vga_device<F: FnOnce(&VgaCharDevice)>(f: F) {
    let driver = get_vga_driver();
    let guard = driver.read();
    let fsdriver = &**guard;
    let vgadriver = fsdriver.as_any().downcast_ref::<VgaDriver>().unwrap();

    let device = vgadriver.get_device();
    let dguard = device.read();
    let device = &**dguard;
    let vgadevice = device.as_any().downcast_ref::<VgaCharDevice>().unwrap();

    f(vgadevice);
}

pub fn use_vga_device_mut<F: FnOnce(&mut VgaCharDevice)>(f: F) {
    let driver = get_vga_driver();
    let guard = driver.read();
    let fsdriver = &**guard;
    let vgadriver = fsdriver.as_any().downcast_ref::<VgaDriver>().unwrap();

    let device = vgadriver.get_device();
    let mut dguard = device.write();
    let device = &mut **dguard;
    let vgadevice = device.as_any_mut().downcast_mut::<VgaCharDevice>().unwrap();

    f(vgadevice)
}
