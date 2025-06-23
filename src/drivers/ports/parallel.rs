use alloc::{boxed::Box, sync::Arc};

use crate::{
    bios::get_bda,
    debuggable_bitset_enum,
    drivers::{
        fs::virt::devfs::{DevFs, VirtualDeviceFile, VirtualDeviceFileProvider},
        vfs::{
            arcrwb_new_from_box, Arcrwb, FileStat, FileSystem, SeekPosition, VfsError, VfsFile,
            VfsFileKind, VfsSpecificFileData, FLAG_PHYSICAL_CHARACTER_DEVICE, FLAG_SYSTEM,
            FLAG_VIRTUAL, OPEN_MODE_FAIL_IF_EXISTS,
        },
    },
    io::{inb, iowait, outb},
    permissions,
};

pub const DATA_REG: u16 = 0;
pub const STATUS_REG: u16 = 1;
pub const CONTROL_REG: u16 = 2;

debuggable_bitset_enum! {
    u8,
    pub enum StatusFlag {
        Reserved0 = 1,
        Reserved1 = 2,
        IRQ = 4,
        NotError = 8,
        SelectIn = 16,
        PaperOut = 32,
        NotAck = 64,
        NotBusy = 128,
    },
    StatusFlags
}

debuggable_bitset_enum! {
    u8,
    pub enum ControlFlag {
        Strobe = 1,
        AutoLF = 2,
        Initialize = 4,
        Select = 8,
        IrqAck = 16,
        Bidi = 32,
        Unused0 = 64,
        Unused1 = 128,
    },
    ControlFlags
}

#[derive(Debug, Clone, Copy)]
pub struct ParallelPort {
    pub parallel_idx: u8,
    pub base_port: u16,
}

impl ParallelPort {
    pub fn new(parallel_idx: u8, base_port: u16) -> ParallelPort {
        ParallelPort {
            parallel_idx,
            base_port,
        }
    }

    #[inline(always)]
    unsafe fn get_status(&self) -> StatusFlags {
        StatusFlags::from(inb(self.base_port + STATUS_REG))
    }

    #[inline(always)]
    unsafe fn get_control(&self) -> ControlFlags {
        ControlFlags::from(inb(self.base_port + CONTROL_REG))
    }

    #[inline(always)]
    unsafe fn put_data(&self, data: u8) {
        outb(self.base_port + DATA_REG, data);
    }

    #[inline(always)]
    unsafe fn pulse_control(&self) {
        outb(
            self.base_port + CONTROL_REG,
            self.get_control().set(ControlFlag::Strobe).get(),
        );
        while !self.get_status().has(StatusFlag::NotAck) {
            // TODO: Maybe schedule a task here?
            iowait();
        }
        outb(
            self.base_port + CONTROL_REG,
            self.get_control().unset(ControlFlag::Strobe).get(),
        );
    }

    /// # Safety
    /// Caller must ensure the code is running with correct IOPL
    pub unsafe fn write_byte(&self, byte: u8) {
        while !self.get_status().has(StatusFlag::NotBusy) {
            // TODO: Maybe schedule a task here?
            iowait();
        }

        self.put_data(byte);
        self.pulse_control();
    }
}

static mut LPTS: Option<[Option<ParallelPort>; 3]> = None;

#[allow(static_mut_refs)]
pub fn get_lpts() -> [Option<ParallelPort>; 3] {
    unsafe {
        match &LPTS {
            Some(lpts) => *lpts,
            None => {
                let bda = get_bda();
                let mut lpts = [None, None, None];
                for (i, (io_base, lpt)) in { bda.lpt_parallel_base_io }
                    .iter()
                    .zip(lpts.iter_mut())
                    .enumerate()
                {
                    if *io_base != 0 {
                        *lpt = Some(ParallelPort::new((i + 1) as u8, *io_base));
                    }
                }
                LPTS = Some(lpts);
                lpts
            }
        }
    }
}

#[allow(static_mut_refs)]
pub fn lpt1() -> Option<ParallelPort> {
    unsafe {
        if let Some(lpts) = &LPTS {
            lpts[0]
        } else {
            get_lpts()[0]
        }
    }
}

#[allow(static_mut_refs)]
pub fn lpt2() -> Option<ParallelPort> {
    unsafe {
        if let Some(lpts) = &LPTS {
            lpts[1]
        } else {
            get_lpts()[1]
        }
    }
}

#[allow(static_mut_refs)]
pub fn lpt3() -> Option<ParallelPort> {
    unsafe {
        if let Some(lpts) = &LPTS {
            lpts[2]
        } else {
            get_lpts()[2]
        }
    }
}

#[derive(Debug)]
pub struct LptProvider {
    lpt: ParallelPort,
    devfs_os_id: u64,
}

impl VirtualDeviceFileProvider for LptProvider {
    fn open(&mut self, mode: u64) -> Result<Arcrwb<dyn VirtualDeviceFile>, VfsError> {
        if mode & OPEN_MODE_FAIL_IF_EXISTS != 0 {
            Err(VfsError::FileAlreadyExists)
        } else {
            Ok(arcrwb_new_from_box(Box::new(self.lpt)))
        }
    }

    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            created_at: 0,
            modified_at: 0,
            permissions: permissions!(Owner:Write, Group:Write).to_u64(), // TODO: implement read
            is_file: true,
            is_directory: false,
            is_symlink: false,
            owner_id: 0,
            group_id: 0,
            flags: FLAG_VIRTUAL | FLAG_SYSTEM | FLAG_PHYSICAL_CHARACTER_DEVICE,
        })
    }

    fn vfs_file(&self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::File,
            alloc::vec!['l', 'p', 't', (b'0' + self.lpt.parallel_idx) as char],
            0,
            self.devfs_os_id,
            self.devfs_os_id,
            Arc::new(VfsSpecificFileData),
        ))
    }
}

impl VirtualDeviceFile for ParallelPort {
    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            created_at: 0,
            modified_at: 0,
            permissions: permissions!(Owner:Write, Group:Write).to_u64(), // TODO: implement read
            is_file: true,
            is_directory: false,
            is_symlink: false,
            owner_id: 0,
            group_id: 0,
            flags: FLAG_VIRTUAL | FLAG_SYSTEM | FLAG_PHYSICAL_CHARACTER_DEVICE,
        })
    }

    fn close(&mut self) -> Result<(), VfsError> {
        Ok(())
    }

    fn seek(&mut self, position: SeekPosition) -> Result<u64, VfsError> {
        if matches!(
            position,
            SeekPosition::FromStart(0) | SeekPosition::FromCurrent(0) | SeekPosition::FromEnd(0)
        ) {
            Ok(0)
        } else {
            Err(VfsError::InvalidSeekPosition)
        }
    }

    fn pos(&self) -> Result<u64, VfsError> {
        Ok(0)
    }

    fn truncate(&mut self) -> Result<u64, VfsError> {
        Ok(0)
    }

    fn read(&mut self, _buf: &mut [u8]) -> Result<u64, VfsError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<u64, VfsError> {
        let count = buf.len();

        for byte in buf {
            unsafe { self.write_byte(*byte) };
        }

        Ok(count as u64)
    }
}

pub fn init_lpt_files(devfs: &mut DevFs) {
    let lpt1 = lpt1();
    let lpt2 = lpt2();
    let lpt3 = lpt3();

    let osid = devfs.os_id();

    if let Some(lpt1) = lpt1 {
        devfs.insert_vfile(
            arcrwb_new_from_box(Box::new(LptProvider {
                lpt: lpt1,
                devfs_os_id: osid,
            })),
            &['l', 'p', 't', '1'],
        );
    }
    if let Some(lpt2) = lpt2 {
        devfs.insert_vfile(
            arcrwb_new_from_box(Box::new(LptProvider {
                lpt: lpt2,
                devfs_os_id: osid,
            })),
            &['l', 'p', 't', '2'],
        );
    }
    if let Some(lpt3) = lpt3 {
        devfs.insert_vfile(
            arcrwb_new_from_box(Box::new(LptProvider {
                lpt: lpt3,
                devfs_os_id: osid,
            })),
            &['l', 'p', 't', '3'],
        );
    }
}
