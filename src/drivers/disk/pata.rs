use crate::io::{inb, inw, outb, outw};

#[derive(Debug, Clone, Copy)]
pub enum PataErrtype {
    DeviceFault,
    DeviceBusy,
    Timeout,
    BadSector,
    Unknown,
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

pub struct PataController {
    drive: PataDrive,
    base_io: u16,    // Base I/O port (like 0x1F0 or 0x170)
    control_io: u16, // Control port (like 0x3F6 or 0x376)
}

impl PataController {
    pub const fn new(bus: PataBus, drive: PataDrive) -> Self {
        let (base_io, control_io) = match bus {
            PataBus::Primary => (0x1F0, 0x3F6),
            PataBus::Secondary => (0x170, 0x376),
        };
        Self {
            drive,
            base_io,
            control_io,
        }
    }

    fn select_drive(&self) {
        let drive_select = match self.drive {
            PataDrive::Master => 0xE0,
            PataDrive::Slave => 0xF0,
        };
        outb(self.base_io + 6, drive_select);
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

    pub fn get_disk_params(&self) -> Result<PataDiskParams, PataErrtype> {
        self.select_drive();
        if !self.wait_busy() {
            return Err(PataErrtype::DeviceBusy);
        }

        outb(
            self.base_io + 6,
            match self.drive {
                PataDrive::Master => 0xA0,
                PataDrive::Slave => 0xB0,
            },
        );
        outb(self.base_io + 7, 0xEC); // IDENTIFY DEVICE

        if !self.wait_drq() {
            return Err(PataErrtype::Timeout);
        }

        let mut identify_data = [0u16; 256];
        for v in identify_data.iter_mut() {
            *v = inw(self.base_io);
        }

        let sector_count = ((identify_data[61] as u64) << 16) | (identify_data[60] as u64);

        Ok(PataDiskParams { sector_count })
    }
}
