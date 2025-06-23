use alloc::{boxed::Box, sync::Arc};

use crate::{
    drivers::{
        fs::virt::devfs::{VirtualDeviceFile, VirtualDeviceFileProvider},
        vfs::{
            arcrwb_new_from_box, Arcrwb, FileStat, SeekPosition, VfsError, VfsFile, VfsFileKind,
            VfsSpecificFileData, FLAG_SYSTEM, FLAG_VIRTUAL, FLAG_VIRTUAL_CHARACTER_DEVICE,
            OPEN_MODE_FAIL_IF_EXISTS,
        },
    },
    permissions,
};

#[derive(Debug)]
pub struct DevNull;

#[derive(Debug)]
pub struct DevNullProvider {
    devfs_os_id: u64,
}

impl DevNullProvider {
    pub fn new(devfs_os_id: u64) -> Self {
        Self { devfs_os_id }
    }
}

impl VirtualDeviceFileProvider for DevNullProvider {
    fn open(&mut self, mode: u64) -> Result<Arcrwb<dyn VirtualDeviceFile>, VfsError> {
        if mode & OPEN_MODE_FAIL_IF_EXISTS != 0 {
            Err(VfsError::FileAlreadyExists)
        } else {
            Ok(arcrwb_new_from_box(Box::new(DevNull)))
        }
    }

    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            is_directory: false,
            is_symlink: false,
            is_file: true,
            permissions: permissions!(Owner:Read, Owner:Write, Group:Read, Group:Write, Other:Read, Other:Write).to_u64(),
            owner_id: 0,
            group_id: 0,
            created_at: 0,
            modified_at: 0,
            flags: FLAG_VIRTUAL | FLAG_VIRTUAL_CHARACTER_DEVICE | FLAG_SYSTEM,
        })
    }

    fn vfs_file(&self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::File,
            "null".chars().collect(),
            0,
            self.devfs_os_id,
            self.devfs_os_id,
            Arc::new(VfsSpecificFileData),
        ))
    }
}

impl VirtualDeviceFile for DevNull {
    fn stat(&self) -> Result<FileStat, VfsError> {
        Ok(FileStat {
            size: 0,
            is_directory: false,
            is_symlink: false,
            is_file: true,
            permissions: permissions!(Owner:Read, Owner:Write, Group:Read, Group:Write, Other:Read, Other:Write).to_u64(),
            owner_id: 0,
            group_id: 0,
            created_at: 0,
            modified_at: 0,
            flags: FLAG_VIRTUAL | FLAG_VIRTUAL_CHARACTER_DEVICE | FLAG_SYSTEM,
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
        Ok(buf.len() as u64)
    }
}
