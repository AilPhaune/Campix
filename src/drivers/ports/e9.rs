use alloc::{boxed::Box, sync::Arc};

use crate::{
    drivers::{
        fs::virt::devfs::{DevFs, VirtualDeviceFile, VirtualDeviceFileProvider},
        vfs::{
            arcrwb_new_from_box, Arcrwb, FileStat, FileSystem, SeekPosition, VfsError, VfsFile,
            VfsFileKind, VfsSpecificFileData, FLAG_PHYSICAL_CHARACTER_DEVICE, FLAG_SYSTEM,
            FLAG_VIRTUAL, OPEN_MODE_FAIL_IF_EXISTS,
        },
    },
    io::outb,
    permissions,
};

#[derive(Debug, Clone)]
pub struct E9 {
    devfs_os_id: u64,
}

impl VirtualDeviceFileProvider for E9 {
    fn open(&mut self, mode: u64) -> Result<Arcrwb<dyn VirtualDeviceFile>, VfsError> {
        if mode & OPEN_MODE_FAIL_IF_EXISTS != 0 {
            Err(VfsError::FileAlreadyExists)
        } else {
            Ok(arcrwb_new_from_box(Box::new(self.clone())))
        }
    }

    fn vfs_file(&self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::File,
            alloc::vec!['e', '9'],
            0,
            self.devfs_os_id,
            self.devfs_os_id,
            Arc::new(VfsSpecificFileData),
        ))
    }

    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            created_at: 0,
            modified_at: 0,
            permissions: permissions!(Owner:Write, Group:Write).to_u64(),
            is_file: true,
            is_directory: false,
            is_symlink: false,
            owner_id: 0,
            group_id: 0,
            flags: FLAG_VIRTUAL | FLAG_SYSTEM | FLAG_PHYSICAL_CHARACTER_DEVICE,
        })
    }
}

impl VirtualDeviceFile for E9 {
    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            created_at: 0,
            modified_at: 0,
            permissions: permissions!(Owner:Write, Group:Write).to_u64(),
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
        for byte in buf {
            outb(0xE9, *byte);
        }
        Ok(buf.len() as u64)
    }
}

pub fn init_e9_file(devfs: &mut DevFs) {
    let osid = devfs.os_id();

    devfs.insert_vfile(
        arcrwb_new_from_box(Box::new(E9 { devfs_os_id: osid })),
        &['e', '9'],
    );
}
