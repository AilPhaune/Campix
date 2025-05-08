use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};
use spin::RwLock;

use crate::{
    drivers::{
        fs::virt::devfs::{fseek_helper, DevFs, DevFsDriver, DevFsHook, DevFsHookKind},
        pci::PciDevice,
        vfs::{
            arcrwb_new_from_box, BlockDevice, FileStat, FileSystem, VfsError, VfsFile, VfsFileKind,
            FLAG_PHYSICAL_BLOCK_DEVICE, OPEN_MODE_APPEND, OPEN_MODE_BINARY, OPEN_MODE_READ,
        },
    },
    io::{inb, inw, outb, outw},
    permissions,
};

pub fn is_pata_device(pci_device: &PciDevice) -> bool {
    pci_device.class == 0x01
        && pci_device.subclass == 0x01
        && (pci_device.prog_if == 0x00
            || pci_device.prog_if == 0x0A
            || pci_device.prog_if == 0x80
            || pci_device.prog_if == 0x8A)
}

#[derive(Debug, Clone, Copy)]
pub enum PataErrtype {
    DeviceFault,
    DeviceBusy,
    Timeout,
    BadSector,
    Unknown,
    NoDevice,
}

#[derive(Debug, Clone, Copy)]
pub struct PataDiskParams {
    pub sector_count: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum PataBus {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy)]
pub enum PataDrive {
    Master,
    Slave,
}

#[derive(Debug)]
pub struct PataController {
    drive: PataDrive,
    base_io: u16,    // Base I/O port (like 0x1F0 or 0x170)
    control_io: u16, // Control port (like 0x3F6 or 0x376)

    identify_data: [u16; 256],
    generation: u64,
}

impl PataController {
    const fn new(bus: PataBus, drive: PataDrive) -> Self {
        let (base_io, control_io) = match bus {
            PataBus::Primary => (0x1F0, 0x3F6),
            PataBus::Secondary => (0x170, 0x376),
        };
        Self {
            drive,
            base_io,
            control_io,
            identify_data: [0; 256],
            generation: 0,
        }
    }

    pub fn is_present(&self) -> bool {
        if self
            .identify_data
            .iter()
            .all(|&word| word == 0xFFFF || word == 0)
        {
            return false;
        }

        let serial_number = &self.identify_data[10..20];
        if serial_number.iter().all(|&word| word == 0) {
            return false;
        }

        let model_string = &self.identify_data[27..47];
        if model_string.iter().all(|&word| word == 0 || word == 0xFF) {
            return false;
        }

        true
    }

    fn select_drive(&self) {
        let drive_select = match self.drive {
            PataDrive::Master => 0xE0,
            PataDrive::Slave => 0xF0,
        };
        outb(self.base_io + 6, drive_select);
        for _ in 0..15 {
            let _ = inb(self.base_io + 7);
        }
    }

    fn wait_busy(&self) -> bool {
        // Wait for BSY bit to clear
        for _ in 0..100000 {
            let status = inb(self.base_io + 7);
            if status & 0x80 == 0 {
                return true;
            }
        }
        false
    }

    fn wait_drq(&self) -> bool {
        // Wait for DRQ bit to set
        for _ in 0..100000 {
            let status = inb(self.base_io + 7);
            if status & 0x08 != 0 {
                return true;
            }
        }
        false
    }

    pub fn get_generation(&self) -> u64 {
        self.generation
    }

    pub fn read_sector(&self, lba: u64, buffer: &mut [u8; 512]) -> Result<(), PataErrtype> {
        self.select_drive();
        if !self.wait_busy() {
            return Err(PataErrtype::DeviceBusy);
        }

        // Send LBA48 commands
        outb(self.control_io, 0x00); // nIEN = 0 (enable interrupts)

        outb(self.base_io + 1, 0); // Features

        outb(self.base_io + 2, ((lba >> 40) & 0xFF) as u8); // Sector Count High
        outb(self.base_io + 3, ((lba >> 24) & 0xFF) as u8); // LBA High
        outb(self.base_io + 4, ((lba >> 32) & 0xFF) as u8); // LBA Mid
        outb(self.base_io + 5, ((lba >> 40) & 0xFF) as u8); // LBA Low

        outb(self.base_io + 2, 1); // Sector Count Low (read 1 sector)
        outb(self.base_io + 3, (lba & 0xFF) as u8);
        outb(self.base_io + 4, ((lba >> 8) & 0xFF) as u8);
        outb(self.base_io + 5, ((lba >> 16) & 0xFF) as u8);

        outb(self.base_io + 7, 0x24); // READ SECTORS EXT (0x24)

        if !self.wait_drq() {
            return Err(PataErrtype::Timeout);
        }

        unsafe {
            let data_port = self.base_io;
            let buf_ptr = buffer.as_mut_ptr() as *mut u16;
            for i in 0..256 {
                *buf_ptr.add(i) = inw(data_port);
            }
        }
        Ok(())
    }

    pub fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), PataErrtype> {
        self.select_drive();
        if !self.wait_busy() {
            return Err(PataErrtype::DeviceBusy);
        }

        // Send LBA48 commands
        outb(self.control_io, 0x00); // nIEN = 0 (enable interrupts)

        outb(self.base_io + 1, 0); // Features

        outb(self.base_io + 2, ((lba >> 40) & 0xFF) as u8);
        outb(self.base_io + 3, ((lba >> 24) & 0xFF) as u8);
        outb(self.base_io + 4, ((lba >> 32) & 0xFF) as u8);
        outb(self.base_io + 5, ((lba >> 40) & 0xFF) as u8);

        outb(self.base_io + 2, 1); // Sector Count
        outb(self.base_io + 3, (lba & 0xFF) as u8);
        outb(self.base_io + 4, ((lba >> 8) & 0xFF) as u8);
        outb(self.base_io + 5, ((lba >> 16) & 0xFF) as u8);

        outb(self.base_io + 7, 0x34); // WRITE SECTORS EXT (0x34)

        if !self.wait_drq() {
            return Err(PataErrtype::Timeout);
        }

        unsafe {
            let data_port = self.base_io;
            let buf_ptr = data.as_ptr() as *const u16;
            for i in 0..256 {
                outw(data_port, *buf_ptr.add(i));
            }
        }

        Ok(())
    }

    pub fn identify(&mut self) -> Result<(), PataErrtype> {
        self.select_drive();

        // Optional: Check status register. If 0x00, no drive is present
        let status = inb(self.base_io + 7);
        if status == 0x00 {
            return Err(PataErrtype::NoDevice);
        }

        // Clear registers as per ATA spec
        outb(self.base_io + 2, 0); // sector count
        outb(self.base_io + 3, 0); // LBA low
        outb(self.base_io + 4, 0); // LBA mid
        outb(self.base_io + 5, 0); // LBA high

        // Send IDENTIFY command
        outb(self.base_io + 7, 0xEC);

        // Wait for BSY to clear and DRQ to set
        if !self.wait_drq() {
            return Err(PataErrtype::Timeout);
        }

        // Read IDENTIFY data
        for word in self.identify_data.iter_mut() {
            *word = inw(self.base_io);
        }

        Ok(())
    }

    pub fn get_disk_params(&self) -> PataDiskParams {
        let sector_count =
            ((self.identify_data[61] as u64) << 16) | (self.identify_data[60] as u64);

        PataDiskParams { sector_count }
    }

    pub fn size_bytes(&self) -> u64 {
        self.get_disk_params().sector_count * 512
    }
}

#[derive(Debug)]
pub struct PataDevfsDriver {
    pci_device: PciDevice,
    handles: BTreeSet<u64>,

    controller_pm: Arc<RwLock<PataController>>,
    controller_ps: Arc<RwLock<PataController>>,
    controller_sm: Arc<RwLock<PataController>>,
    controller_ss: Arc<RwLock<PataController>>,
}

impl PataDevfsDriver {
    pub fn new(pci_device: PciDevice) -> Self {
        let driver = Self {
            pci_device,
            handles: BTreeSet::new(),
            controller_pm: Arc::new(RwLock::new(PataController::new(
                PataBus::Primary,
                PataDrive::Master,
            ))),
            controller_ps: Arc::new(RwLock::new(PataController::new(
                PataBus::Primary,
                PataDrive::Slave,
            ))),
            controller_sm: Arc::new(RwLock::new(PataController::new(
                PataBus::Secondary,
                PataDrive::Master,
            ))),
            controller_ss: Arc::new(RwLock::new(PataController::new(
                PataBus::Secondary,
                PataDrive::Slave,
            ))),
        };
        let _ = driver.controller_pm.write().identify();
        let _ = driver.controller_ps.write().identify();
        let _ = driver.controller_sm.write().identify();
        let _ = driver.controller_ss.write().identify();
        driver
    }

    pub fn get_pci_device(&self) -> PciDevice {
        self.pci_device
    }
}

#[derive(Debug)]
struct PataBlockDevice {
    controller: Arc<RwLock<PataController>>,
}

impl BlockDevice for PataBlockDevice {
    fn get_generation(&self) -> u64 {
        self.controller.read().get_generation()
    }

    fn get_block_size(&self) -> u64 {
        512
    }

    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if buf.len() < 512 {
            return Err(VfsError::BadBufferSize);
        }
        let mut data: [u8; 512] = [0; 512];
        self.controller
            .read()
            .read_sector(lba, &mut data)
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;
        buf.copy_from_slice(&data);
        Ok(512)
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if buf.len() != 512 {
            return Err(VfsError::BadBufferSize);
        }
        let mut data: [u8; 512] = [0; 512];
        data.copy_from_slice(buf);
        self.controller
            .write()
            .write_sector(lba, &data)
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;
        Ok(512)
    }
}

const PATA: u64 = u64::from_be_bytes([0, 0, 0, 0, b'p', b'a', b't', b'a']);

#[derive(Debug, Clone)]
struct PataFsFileHandle {
    mode: u64,
    controller: Arc<RwLock<PataController>>,
    position: u64,
    last_sector: Option<u64>,
    sector_cache: [u8; 512],
    generation: u64,
}

impl DevFsDriver for PataDevfsDriver {
    fn driver_id(&self) -> u64 {
        PATA
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
        for (name, controller) in [
            (
                "pata_pm".chars().collect::<Vec<_>>(),
                self.controller_pm.clone(),
            ),
            (
                "pata_ps".chars().collect::<Vec<_>>(),
                self.controller_ps.clone(),
            ),
            (
                "pata_sm".chars().collect::<Vec<_>>(),
                self.controller_sm.clone(),
            ),
            (
                "pata_ss".chars().collect::<Vec<_>>(),
                self.controller_ss.clone(),
            ),
        ] {
            let guard = controller.write();
            if !guard.is_present() {
                dev_fs.remove_hook(&name);
                continue;
            }
            let file = VfsFile::new(
                VfsFileKind::BlockDevice {
                    device: arcrwb_new_from_box(Box::new(PataBlockDevice {
                        controller: controller.clone(),
                    })),
                },
                name.clone(),
                0,
                dev_fs.os_id(),
                dev_fs.os_id(),
            );
            dev_fs.replace_hook(
                name,
                self.driver_id(),
                file,
                DevFsHookKind::Device,
                guard.generation,
            );
        }
        Ok(())
    }

    fn handles_device(&self, _dev_fs: &mut DevFs, pci_device: &PciDevice) -> bool {
        self.pci_device == *pci_device
    }

    fn fopen(
        &mut self,
        dev_fs: &mut DevFs,
        hook: Arc<DevFsHook>,
        mode: u64,
    ) -> Result<u64, VfsError> {
        let controller = if hook.file.name().get(0..7) == Some(&['p', 'a', 't', 'a', '_', 'p', 'm'])
        {
            &self.controller_pm
        } else if hook.file.name().get(0..7) == Some(&['p', 'a', 't', 'a', '_', 'p', 's']) {
            &self.controller_ps
        } else if hook.file.name().get(0..7) == Some(&['p', 'a', 't', 'a', '_', 's', 'm']) {
            &self.controller_sm
        } else if hook.file.name().get(0..7) == Some(&['p', 'a', 't', 'a', '_', 's', 's']) {
            &self.controller_ss
        } else {
            return Err(VfsError::PathNotFound);
        };

        if !controller.read().is_present() {
            return Err(VfsError::PathNotFound);
        }

        if mode & OPEN_MODE_APPEND != 0 {
            return Err(VfsError::InvalidOpenMode);
        }
        if mode & OPEN_MODE_BINARY == 0 {
            return Err(VfsError::InvalidOpenMode);
        }

        let handle_data = PataFsFileHandle {
            mode,
            controller: controller.clone(),
            last_sector: None,
            position: 0,
            sector_cache: [0; 512],
            generation: hook.generation,
        };
        let handle = dev_fs.alloc_file_handle(handle_data, hook);

        self.handles.insert(handle);
        Ok(handle)
    }

    fn fclose(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        self.handles.remove(&handle);
        dev_fs.dealloc_file_handle::<PataFsFileHandle>(handle);
        Ok(())
    }

    fn fflush(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }
        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let controller = handle_data.controller.read();
        if controller.generation != handle_data.generation {
            return Err(VfsError::BadHandle);
        }

        Ok(())
    }

    fn fsync(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }

        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let mut controller = handle_data.controller.write();
        if controller.generation != handle_data.generation {
            return Err(VfsError::BadHandle);
        }

        if !controller.is_present() {
            return Err(VfsError::PathNotFound);
        }

        controller.generation += 1;
        handle_data.generation = controller.generation;

        Ok(())
    }

    fn fread(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }

        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let controller = handle_data.controller.read();

        if !controller.is_present() {
            return Err(VfsError::PathNotFound);
        }

        if handle_data.mode & OPEN_MODE_READ == 0 {
            return Err(VfsError::ActionNotAllowed);
        }

        let mut bytes_read = 0;
        let to_read = buf
            .len()
            .min((controller.size_bytes() - handle_data.position) as usize);
        let mut sector = handle_data.position / 512;

        while bytes_read < to_read {
            if
            /* TODO: or if it's not write-locked */
            handle_data.last_sector != Some(sector) {
                controller
                    .read_sector(sector, &mut handle_data.sector_cache)
                    .map_err(|e| VfsError::DriverError(Box::new(e)))?;
                handle_data.last_sector = Some(sector);
            }

            let sector_offset = (handle_data.position % 512) as usize;
            let remaining_in_sector = 512 - sector_offset;
            let remaining_to_read = to_read - bytes_read;
            let to_copy = remaining_in_sector.min(remaining_to_read);

            buf[bytes_read..bytes_read + to_copy]
                .copy_from_slice(&handle_data.sector_cache[sector_offset..sector_offset + to_copy]);

            handle_data.position += to_copy as u64;
            bytes_read += to_copy;
            sector = handle_data.position / 512;
        }
        Ok(bytes_read as u64)
    }

    fn fwrite(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }

        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        if (handle_data.mode & OPEN_MODE_READ) == 0 {
            return Err(VfsError::ActionNotAllowed);
        }

        let mut controller = handle_data.controller.write();
        if controller.generation != handle_data.generation {
            return Err(VfsError::BadHandle);
        }

        if !controller.is_present() {
            return Err(VfsError::PathNotFound);
        }

        let mut bytes_written = 0;
        let to_write = buf
            .len()
            .min((controller.size_bytes() - handle_data.position) as usize);
        let mut sector = handle_data.position / 512;

        while bytes_written < to_write {
            let sector_offset = (handle_data.position % 512) as usize;
            let remaining_in_sector = 512 - sector_offset;
            let remaining_to_write = to_write - bytes_written;
            let to_copy = remaining_in_sector.min(remaining_to_write);

            // Read back the sector if we're not overwriting all of its data
            // TODO: if it's write-locked and already stores the sector data, no need to read it back
            if to_copy != 512 {
                controller
                    .read_sector(sector, &mut handle_data.sector_cache)
                    .map_err(|e| VfsError::DriverError(Box::new(e)))?;
            }
            handle_data.last_sector = Some(sector);

            handle_data.sector_cache[sector_offset..sector_offset + to_copy]
                .copy_from_slice(&buf[bytes_written..bytes_written + to_copy]);

            controller
                .write_sector(sector, &handle_data.sector_cache)
                .map_err(|e| VfsError::DriverError(Box::new(e)))?;

            handle_data.position += to_copy as u64;
            bytes_written += to_copy;
            sector = handle_data.position / 512;
        }
        Ok(bytes_written as u64)
    }

    fn fseek(
        &mut self,
        dev_fs: &mut DevFs,
        handle: u64,
        position: crate::drivers::vfs::SeekPosition,
    ) -> Result<u64, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }

        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let len = {
            let controller = handle_data.controller.read();
            if !controller.is_present() {
                return Err(VfsError::PathNotFound);
            }
            controller.size_bytes()
            // Drop the controller as early as possible to let other threads access it
        };

        handle_data.position = fseek_helper(position, handle_data.position, len)
            .ok_or(VfsError::InvalidSeekPosition)?;

        Ok(handle_data.position)
    }

    fn fstat(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<FileStat, VfsError> {
        if !self.handles.contains(&handle) {
            return Err(VfsError::BadHandle);
        }

        let handle_data = unsafe {
            &mut *(dev_fs
                .get_handle_data::<PataFsFileHandle>(handle)
                .ok_or(VfsError::BadHandle)?)
        };
        let len = {
            let controller = handle_data.controller.read();
            if controller.generation != handle_data.generation {
                return Err(VfsError::BadHandle);
            }
            if !controller.is_present() {
                return Err(VfsError::PathNotFound);
            }
            controller.size_bytes()
            // Drop the controller as early as possible to let other threads access it
        };

        Ok(FileStat {
            size: len,
            is_directory: false,
            is_symlink: false,
            permissions: permissions!(Owner:Read, Owner:Write).to_u32(),
            owner_id: 0,
            group_id: 0,
            created_at: 0,
            modified_at: 0,
            flags: FLAG_PHYSICAL_BLOCK_DEVICE,
        })
    }
}
