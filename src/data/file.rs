use alloc::vec::Vec;

use crate::drivers::vfs::{get_vfs, Arcrwb, FileStat, FileSystem, SeekPosition, VfsError, VfsFile};

pub struct File {
    path: Vec<char>,
    fs: Arcrwb<dyn FileSystem>,
    file: VfsFile,
    handle: u64,
}

impl File {
    pub fn open(path: &str, mode: u64) -> Result<File, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        let fs = get_vfs();
        let guard = fs.read();
        let file = guard.get_file(&path)?;
        let fs = guard
            .get_fs_by_id(file.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = fs.write();
        let handle = guard.fopen(&file, mode)?;
        drop(guard);

        Ok(File {
            path,
            fs,
            file,
            handle,
        })
    }

    fn open_entry(entry: &DirectoryEntry, mode: u64) -> Result<File, VfsError> {
        let fs = get_vfs();
        let guard = fs.read();
        let sub_fs = guard
            .get_fs_by_id(entry.entry.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = sub_fs.write();
        let handle = guard.fopen(&entry.entry, mode)?;
        drop(guard);

        Ok(File {
            path: entry.full_name.clone(),
            fs: sub_fs,
            file: entry.entry.clone(),
            handle,
        })
    }

    pub fn stats(&self) -> Result<FileStat, VfsError> {
        let guard = &self.fs.read();
        guard.fstat(self.handle)
    }

    pub fn get_path(&self) -> &Vec<char> {
        &self.path
    }

    pub fn get_vfs_file(&self) -> &VfsFile {
        &self.file
    }

    /// # Safety
    /// Caller is responsible for what they do with the handle
    pub unsafe fn get_handle(&self) -> u64 {
        self.handle
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fwrite(self.handle, buf)
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fread(self.handle, buf)
    }

    pub fn seek(&self, position: SeekPosition) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fseek(self.handle, position)
    }

    fn _close(&mut self) -> Result<(), VfsError> {
        let mut guard = self.fs.write();
        guard.fclose(self.handle)
    }

    pub fn close(mut self) -> Result<(), VfsError> {
        self._close()
    }

    pub fn sync(&mut self) -> Result<(), VfsError> {
        let mut guard = self.fs.write();
        guard.fsync(self.handle)
    }

    pub fn flush(&mut self) -> Result<(), VfsError> {
        let mut guard = self.fs.write();
        guard.fflush(self.handle)
    }

    pub fn list_directory(path: &str) -> Result<Vec<DirectoryEntry>, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        let fs = get_vfs();
        let guard: &dyn FileSystem = &**fs.read();
        let directory = guard.get_file(&path)?;
        if directory.is_mount_point() {
            let fs = directory
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?;
            let guard = &**fs.read();
            let directory = guard.get_root();
            return Ok(guard
                .list_children(&directory)?
                .iter()
                .map(|entry| DirectoryEntry {
                    full_name: [&path, &['/'] as &[char], entry.name()].concat(),
                    entry: entry.clone(),
                })
                .collect::<Vec<_>>());
        }
        if !directory.is_directory() {
            return Err(VfsError::NotDirectory);
        }
        Ok(guard
            .list_children(&directory)?
            .iter()
            .map(|entry| DirectoryEntry {
                full_name: [&path, &['/'] as &[char], entry.name()].concat(),
                entry: entry.clone(),
            })
            .collect::<Vec<_>>())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = self._close();
    }
}

pub struct DirectoryEntry {
    full_name: Vec<char>,
    entry: VfsFile,
}

impl DirectoryEntry {
    pub fn name(&self) -> &[char] {
        self.entry.name()
    }

    pub fn full_name(&self) -> &[char] {
        &self.full_name
    }

    pub fn open(&self, mode: u64) -> Result<File, VfsError> {
        File::open_entry(self, mode)
    }
}
