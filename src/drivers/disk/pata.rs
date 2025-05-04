use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::RwLock;

use crate::{
    drivers::{
        fs::virt::devfs::{DevFS, DevFsDriver},
        pci::PciDevice,
        vfs::{arcrwb_new_from_box, BlockDevice, FileSystem, VfsError, VfsFile, VfsFileKind},
    },
    io::{inb, inw, outb, outw},
};

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
}

#[derive(Debug)]
pub struct PataDevfsDriver {
    controller_pm: Arc<RwLock<PataController>>,
    controller_ps: Arc<RwLock<PataController>>,
    controller_sm: Arc<RwLock<PataController>>,
    controller_ss: Arc<RwLock<PataController>>,
}

impl Default for PataDevfsDriver {
    fn default() -> Self {
        let driver = Self {
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
}

#[derive(Debug)]
struct PataBlockDevice(Arc<RwLock<PataController>>);

impl BlockDevice for PataBlockDevice {
    fn get_block_size(&self) -> u64 {
        512
    }

    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if buf.len() < 512 {
            return Err(VfsError::BadBufferSize);
        }
        let mut data: [u8; 512] = [0; 512];
        self.0
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
        self.0
            .write()
            .write_sector(lba, &data)
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;
        Ok(512)
    }
}

const PATA: u64 = u64::from_be_bytes([0, 0, 0, 0, b'p', b'a', b't', b'a']);

impl DevFsDriver for PataDevfsDriver {
    fn driver_id(&self) -> u64 {
        PATA
    }

    fn get_device_files(
        &self,
        devfs: &mut DevFS,
        device: &PciDevice,
    ) -> Result<Vec<VfsFile>, VfsError> {
        if !self.handles_device(devfs, device) {
            return Err(VfsError::ActionNotAllowed);
        }
        let mut files: Vec<(Vec<char>, Arc<RwLock<PataController>>)> = Vec::new();
        if self.controller_pm.read().is_present() {
            files.push((
                "pata_pm".chars().collect::<Vec<_>>(),
                self.controller_pm.clone(),
            ));
        }
        if self.controller_ps.read().is_present() {
            files.push((
                "pata_ps".chars().collect::<Vec<_>>(),
                self.controller_ps.clone(),
            ));
        }
        if self.controller_sm.read().is_present() {
            files.push((
                "pata_sm".chars().collect::<Vec<_>>(),
                self.controller_sm.clone(),
            ));
        }
        if self.controller_ss.read().is_present() {
            files.push((
                "pata_ss".chars().collect::<Vec<_>>(),
                self.controller_ss.clone(),
            ));
        }

        Ok(files
            .iter()
            .map(|(name, controller)| {
                let block_device = VfsFileKind::BlockDevice {
                    device: arcrwb_new_from_box(Box::new(PataBlockDevice(controller.clone()))),
                };
                VfsFile::new(block_device, name.clone(), 0, devfs.os_id(), devfs.os_id())
            })
            .collect::<Vec<_>>())
    }

    fn handles_device(&self, _dev_fs: &mut DevFS, pci_device: &PciDevice) -> bool {
        pci_device.class == 0x01
            && pci_device.subclass == 0x01
            && (pci_device.prog_if == 0x00
                || pci_device.prog_if == 0x0A
                || pci_device.prog_if == 0x80
                || pci_device.prog_if == 0x8A)
    }
}
