use core::fmt::Debug;

use alloc::{string::String, vec::Vec};

use crate::{
    data::permissions::Permissions,
    drivers::vfs::{
        get_vfs, Arcrwb, FileStat, FileSystem, PathTraverse, SeekPosition, VfsError, VfsFile,
        VfsFileKind,
    },
};

pub struct File {
    mode: u64,
    path: Vec<char>,
    fs: Arcrwb<dyn FileSystem>,
    file: VfsFile,
    handle: u64,
}

impl Debug for File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("File")
            .field("mode", &self.mode)
            .field("path", &self.path.iter().collect::<String>())
            .field("handle", &self.handle)
            .finish()
    }
}

impl File {
    // TODO: Add create_perms on FileSystem interface
    pub fn open(path: &str, mode: u64, _create_perms: Permissions) -> Result<File, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        let fs = get_vfs();
        let mut guard = fs.write();
        let file = guard.get_file(&path)?;
        let fs = guard
            .get_fs_by_id(file.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = fs.write();
        let handle = guard.fopen(&file, mode)?;
        drop(guard);

        Ok(File {
            mode,
            path,
            fs,
            file,
            handle,
        })
    }

    pub fn open_raw(
        path: &[char],
        mode: u64,
        _create_perms: Permissions,
    ) -> Result<(Arcrwb<dyn FileSystem>, u64, VfsFile), VfsError> {
        let fs = get_vfs();
        let mut guard = fs.write();
        let file = guard.get_file(path)?;
        let fs = guard
            .get_fs_by_id(file.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = fs.write();
        let handle = guard.fopen(&file, mode)?;
        drop(guard);
        Ok((fs, handle, file))
    }

    pub fn get_stats(path: &str) -> Result<Option<FileStat>, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        let fs = get_vfs();
        let mut guard = fs.write();
        guard.get_stats(&path)
    }

    pub fn get_stats0(path: &[char]) -> Result<Option<FileStat>, VfsError> {
        let fs = get_vfs();
        let mut guard = fs.write();
        guard.get_stats(path)
    }

    pub fn create(path: &str, mode: u64, _perms: Permissions) -> Result<File, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        let name_start = path
            .iter()
            .rposition(|c| *c == '/')
            .ok_or(VfsError::InvalidArgument)?;

        let dirname = &path[..name_start];
        let filename = &path[name_start + 1..];
        if filename.is_empty() {
            return Err(VfsError::InvalidArgument);
        }

        let fs = get_vfs();
        let mut guard = fs.write();

        let directory = guard.get_file(dirname)?;

        let fs = guard
            .get_fs_by_id(directory.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = fs.write();
        // TODO: Use perms
        let file = guard.create_child(&directory, filename, VfsFileKind::File)?;
        let handle = guard.fopen(&file, mode)?;
        drop(guard);

        Ok(File {
            mode,
            path,
            fs,
            file,
            handle,
        })
    }

    pub fn delete(path: &str) -> Result<(), VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        Self::delete0(&path)
    }

    pub fn delete0(path: &[char]) -> Result<(), VfsError> {
        let fs = get_vfs();
        let mut guard = fs.write();
        let file = guard.get_file(path)?;
        let fs = guard
            .get_fs_by_id(file.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        drop(guard);
        let mut guard = fs.write();
        guard.delete_file(&file)?;
        drop(guard);
        Ok(())
    }

    pub fn mkdir0(path: Vec<char>) -> Result<Directory, VfsError> {
        let fs = get_vfs();
        let wguard: &mut dyn FileSystem = &mut **fs.write();
        let mut traverse = PathTraverse::new_owned(&path, wguard)?;
        let mut made_dir = false;
        loop {
            match traverse.find_next() {
                Ok(entry) => {
                    if traverse.is_done() {
                        return if made_dir {
                            DirectoryEntry {
                                full_name: path,
                                entry,
                            }
                            .get_dir()
                        } else {
                            Err(VfsError::FileAlreadyExists)
                        };
                    }
                }
                Err(VfsError::PathNotFound) => {
                    let entry = traverse.mkdir()?;
                    if traverse.is_done() {
                        return DirectoryEntry {
                            full_name: path,
                            entry,
                        }
                        .get_dir();
                    }
                    made_dir = true;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn mkdir(path: &str) -> Result<Directory, VfsError> {
        let path = path.chars().collect::<Vec<char>>();
        Self::mkdir0(path)
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
            mode,
        })
    }

    pub fn get_open_mode(&self) -> u64 {
        self.mode
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

    /// Writes the buffer to the file at the current position, incrementing the position by the amount of bytes written, and returns the number of bytes written
    pub fn write(&mut self, buf: &[u8]) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fwrite(self.handle, buf)
    }

    /// Reads contents from the file at the current position, incrementing the position by the amount of bytes read, and returns the number of bytes read, reading at most enough bytes to fill the buffer
    pub fn read(&self, buf: &mut [u8]) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fread(self.handle, buf)
    }

    /// Seeks to a specific position in the file, returning the new position or an error if the position is invalid
    pub fn seek(&self, position: SeekPosition) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.fseek(self.handle, position)
    }

    /// Truncates the file to the current position, and returns the new position
    pub fn truncate(&mut self) -> Result<u64, VfsError> {
        let mut guard = self.fs.write();
        guard.ftruncate(self.handle)
    }

    /// Closes the file
    /// # Safety
    /// Safe but all subsequent calls to functions on this File will return errors
    pub unsafe fn _close(&mut self) -> Result<(), VfsError> {
        let mut guard = self.fs.write();
        guard.fclose(self.handle)?;
        self.handle = 0;
        Ok(())
    }

    pub fn close(mut self) -> Result<(), VfsError> {
        unsafe { self._close() }
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
        let guard: &mut dyn FileSystem = &mut **fs.write();
        let directory = guard.get_file(&path)?;
        if directory.is_mount_point() {
            let fs = directory
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?;
            let guard = &mut **fs.write();
            let directory = guard.get_root()?;
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
        let _ = unsafe { self._close() };
    }
}

#[derive(Debug)]
pub struct Directory {
    path: String,
}

impl Directory {
    pub fn of(path: &[char]) -> Self {
        let mut value = path.to_vec();
        while let Some(c) = value.last() {
            if *c == '/' {
                value.pop();
            } else {
                break;
            }
        }
        Self {
            path: value.iter().collect::<String>(),
        }
    }

    pub fn list(&self) -> Result<Vec<DirectoryEntry>, VfsError> {
        File::list_directory(&self.path)
    }
}

pub struct DirectoryEntry {
    full_name: Vec<char>,
    entry: VfsFile,
}

impl DirectoryEntry {
    pub fn name(&self) -> &[char] {
        let name = self.entry.name();
        let mut last_idx = name.len() - 1;
        while let Some(c) = name.get(last_idx) {
            if *c == '/' {
                last_idx -= 1;
            } else {
                break;
            }
        }
        last_idx += 1;
        &name[0..last_idx]
    }

    pub fn full_name(&self) -> &[char] {
        &self.full_name
    }

    pub fn open(&self, mode: u64) -> Result<File, VfsError> {
        File::open_entry(self, mode)
    }

    pub fn get_dir(&self) -> Result<Directory, VfsError> {
        match self.entry.kind() {
            VfsFileKind::Directory | VfsFileKind::MountPoint { .. } => {
                Ok(Directory::of(self.full_name()))
            }
            _ => Err(VfsError::NotDirectory),
        }
    }

    pub fn of(path: &str) -> Result<DirectoryEntry, VfsError> {
        let mut path = path.chars().collect::<Vec<char>>();
        while let Some(c) = path.last() {
            if *c == '/' {
                path.pop();
            } else {
                break;
            }
        }
        let fs = get_vfs();
        let guard: &mut dyn FileSystem = &mut **fs.write();
        let directory = guard.get_file(&path)?;
        if directory.is_mount_point() {
            let fs = directory
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?;
            let guard = &mut **fs.write();
            let directory = guard.get_root()?;
            Ok(DirectoryEntry {
                full_name: path,
                entry: directory,
            })
        } else {
            Ok(DirectoryEntry {
                full_name: path,
                entry: directory,
            })
        }
    }
}
