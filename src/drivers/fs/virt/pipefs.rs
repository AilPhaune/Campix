use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec;
use alloc::{boxed::Box, string::String, vec::Vec};
use spin::rwlock::RwLock;

use crate::data::file::File;
use crate::data::{calloc_boxed_slice, decimal_chars_to_u64};
use crate::drivers::vfs::{
    default_get_file_implementation, get_vfs, FileHandleAllocator, FileStat, FsSpecificFileData,
    PipeMode, SeekPosition, Vfs, VfsFileKind, WeakArcrwb, FLAG_SYSTEM, FLAG_VIRTUAL,
    OPEN_MODE_APPEND, OPEN_MODE_CREATE, OPEN_MODE_FAIL_IF_EXISTS, OPEN_MODE_READ, OPEN_MODE_WRITE,
};

use crate::drivers::vfs::{Arcrwb, BlockDevice, FileSystem, VfsError, VfsFile};
use crate::permissions;

#[derive(Debug)]
pub struct Pipe {
    pub data: Box<[u8]>,
    pub data_len: usize,
    pub write_pos: usize,
    pub read_pos: usize,
    pub created_at: u64,
    pub modified_at: u64,

    pub readers: u64,
    pub writers: u64,
    pub closed: bool,
}

macro_rules! impl_pipe_create {
    ($pipe_dir: ident) => {{
        let pipe_vfs_file = $pipe_dir.get_vfs_file();

        let vfs = get_vfs();
        let guard = vfs.write();

        let pipefs = guard
            .get_fs_by_id(pipe_vfs_file.fs())
            .ok_or(VfsError::FileSystemNotMounted)?;
        let mut pipefs_guard = pipefs.write();

        let rfile = pipefs_guard.get_child(pipe_vfs_file, &['r'])?;
        let wfile = pipefs_guard.get_child(pipe_vfs_file, &['w'])?;

        let (Some((_, _, rid)), Some((_, _, wid))) = (rfile.get_pipe(), wfile.get_pipe()) else {
            return Err(VfsError::InvalidArgument);
        };

        if rid != wid {
            return Err(VfsError::UnknownError);
        }

        let r = pipefs_guard.fopen(&rfile, OPEN_MODE_READ)?;
        let w = pipefs_guard.fopen(&wfile, OPEN_MODE_WRITE)?;

        drop(guard);
        drop(pipefs_guard);

        (rid, r, w, pipe_vfs_file, pipefs, rfile, wfile)
    }};
}

impl Pipe {
    pub fn new_anonymous(buf_size: usize) -> Pipe {
        Pipe {
            data: calloc_boxed_slice(buf_size),
            data_len: 0,
            write_pos: 0,
            read_pos: 0,
            created_at: 0,
            modified_at: 0,
            readers: 0,
            writers: 0,
            closed: false,
        }
    }

    /// # Safety
    /// Caller is responsible for what they do with the handles
    pub unsafe fn create_raw_fds() -> Result<(u64, u64, u64, Arcrwb<dyn FileSystem>), VfsError> {
        let pipe_dir = File::mkdir0("/pipes/a".chars().collect::<Vec<char>>())?;
        let (rid, r, w, _, pipe_fs, _, _) = impl_pipe_create!(pipe_dir);
        Ok((rid, r, w, pipe_fs))
    }

    /// Returns (pipe id, read file, write file)
    pub fn create() -> Result<(u64, File, File), VfsError> {
        unsafe {
            let pipe_dir = File::mkdir0("/pipes/a".chars().collect::<Vec<char>>())?;
            let (rid, r, w, pipe_vfs_file, pipefs, rfile, wfile) = impl_pipe_create!(pipe_dir);

            let reader = File::unsafe_from_raw(
                OPEN_MODE_READ,
                [pipe_vfs_file.name(), &['/', 'r']].concat(),
                pipefs.clone(),
                rfile,
                r,
            );
            let writer = File::unsafe_from_raw(
                OPEN_MODE_WRITE,
                [pipe_vfs_file.name(), &['/', 'w']].concat(),
                pipefs.clone(),
                wfile,
                w,
            );
            Ok((rid, reader, writer))
        }
    }

    pub fn readable_bytes(&self) -> usize {
        self.data_len
    }

    pub fn writable_bytes(&self) -> usize {
        self.data.len() - self.data_len
    }

    pub fn is_full(&self) -> bool {
        self.data_len >= self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data_len == 0
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let to_read = self.readable_bytes().min(buf.len());
        if to_read == 0 {
            0
        } else {
            let len = self.data.len();
            let to_end = len - self.read_pos;
            if to_read > to_end {
                buf[..to_end].copy_from_slice(&self.data[self.read_pos..]);
                buf[to_end..to_read].copy_from_slice(&self.data[..to_read - to_end]);
                self.read_pos = to_read - to_end;
            } else {
                buf[..to_read].copy_from_slice(&self.data[self.read_pos..self.read_pos + to_read]);
                self.read_pos = (self.read_pos + to_read) % len;
            }
            self.data_len -= to_read;
            to_read
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> usize {
        let to_write = self.writable_bytes().min(buf.len());
        if to_write == 0 {
            0
        } else {
            let len = self.data.len();
            let to_end = len - self.write_pos;
            if to_write > to_end {
                self.data[self.write_pos..].copy_from_slice(&buf[..to_end]);
                self.data[..to_write - to_end].copy_from_slice(&buf[to_end..to_write]);
                self.write_pos = to_write - to_end;
            } else {
                self.data[self.write_pos..self.write_pos + to_write]
                    .copy_from_slice(&buf[..to_write]);
                self.write_pos = (self.write_pos + to_write) % len;
            }
            self.data_len += to_write;
            to_write
        }
    }
}

#[derive(Debug, Clone)]
pub struct PipeFsHandle {
    pipe: Arcrwb<Pipe>,
    mode: PipeMode,
    pipe_id: u64,
}

#[derive(Debug)]
pub struct PipeFs {
    os_id: u64,
    parent_fs_os_id: u64,
    mnt: Option<VfsFile>,
    root_fs: Option<WeakArcrwb<Vfs>>,

    pipes: BTreeMap<u64, Arcrwb<Pipe>>,
    handles: FileHandleAllocator,

    next_pipe_id: u64,
}

#[derive(Debug)]
pub enum PipeFsSpecificFileData {
    PipefsRoot,
    PipefsDir(u64),
    PipefsRead(u64),
    PipefsWrite(u64),
}

impl FsSpecificFileData for PipeFsSpecificFileData {}

impl FileSystem for PipeFs {
    fn os_id(&mut self) -> u64 {
        self.os_id
    }

    fn fs_type(&mut self) -> String {
        "pipe".to_string()
    }

    fn fs_flush(&mut self) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn host_block_device(&mut self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
    }

    fn get_root(&mut self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::Directory,
            alloc::vec!['/'],
            0,
            self.parent_fs_os_id,
            self.os_id,
            Arc::new(PipeFsSpecificFileData::PipefsRoot),
        ))
    }

    fn get_mount_point(&mut self) -> Result<Option<VfsFile>, VfsError> {
        Ok(Some(
            self.mnt
                .as_ref()
                .ok_or(VfsError::FileSystemNotMounted)?
                .clone(),
        ))
    }

    fn get_child(&mut self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs() != self.os_id {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() == ['/'] {
            let id = decimal_chars_to_u64(child).ok_or(VfsError::PathNotFound)?;

            if self.pipes.contains_key(&id) {
                Ok(VfsFile::new(
                    VfsFileKind::Directory,
                    child.to_vec(),
                    0,
                    self.os_id,
                    self.os_id,
                    Arc::new(PipeFsSpecificFileData::PipefsDir(id)),
                ))
            } else {
                Err(VfsError::PathNotFound)
            }
        } else {
            let d = file.get_fs_specific_data();
            let data = &(*d)
                .as_any()
                .downcast_ref::<PipeFsSpecificFileData>()
                .ok_or(VfsError::FileSystemMismatch)?;

            match data {
                PipeFsSpecificFileData::PipefsDir(id) => {
                    if let Some(pipe) = self.pipes.get(id) {
                        if child == ['r'] {
                            Ok(VfsFile::new(
                                VfsFileKind::Pipe {
                                    pipe: pipe.clone(),
                                    mode: PipeMode::Read,
                                    pipe_id: *id,
                                },
                                child.to_vec(),
                                0,
                                self.os_id,
                                self.os_id,
                                Arc::new(PipeFsSpecificFileData::PipefsRead(*id)),
                            ))
                        } else if child == ['w'] {
                            Ok(VfsFile::new(
                                VfsFileKind::Pipe {
                                    pipe: pipe.clone(),
                                    mode: PipeMode::Write,
                                    pipe_id: *id,
                                },
                                child.to_vec(),
                                0,
                                self.os_id,
                                self.os_id,
                                Arc::new(PipeFsSpecificFileData::PipefsWrite(*id)),
                            ))
                        } else {
                            Err(VfsError::PathNotFound)
                        }
                    } else {
                        Err(VfsError::PathNotFound)
                    }
                }
                _ => Err(VfsError::PathNotFound),
            }
        }
    }

    fn list_children(&mut self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs() != self.os_id {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() == ['/'] {
            let osid = self.os_id;
            Ok(self
                .pipes
                .keys()
                .map(|id| {
                    VfsFile::new(
                        VfsFileKind::Directory,
                        vec!['/'],
                        0,
                        osid,
                        osid,
                        Arc::new(PipeFsSpecificFileData::PipefsDir(*id)),
                    )
                })
                .collect())
        } else {
            let d = file.get_fs_specific_data();
            let data = &(*d)
                .as_any()
                .downcast_ref::<PipeFsSpecificFileData>()
                .ok_or(VfsError::FileSystemMismatch)?;

            match data {
                PipeFsSpecificFileData::PipefsDir(id) => {
                    let osid = self.os_id;
                    if let Some(pipe) = self.pipes.get(id) {
                        Ok(vec![
                            VfsFile::new(
                                VfsFileKind::Pipe {
                                    pipe: pipe.clone(),
                                    mode: PipeMode::Read,
                                    pipe_id: *id,
                                },
                                vec!['r'],
                                0,
                                osid,
                                osid,
                                Arc::new(PipeFsSpecificFileData::PipefsRead(*id)),
                            ),
                            VfsFile::new(
                                VfsFileKind::Pipe {
                                    pipe: pipe.clone(),
                                    mode: PipeMode::Write,
                                    pipe_id: *id,
                                },
                                vec!['w'],
                                0,
                                osid,
                                osid,
                                Arc::new(PipeFsSpecificFileData::PipefsWrite(*id)),
                            ),
                        ])
                    } else {
                        Err(VfsError::PathNotFound)
                    }
                }
                _ => Err(VfsError::PathNotFound),
            }
        }
    }

    default_get_file_implementation!();

    fn get_stats(&mut self, file: &VfsFile) -> Result<FileStat, VfsError> {
        if file.fs() != self.os_id {
            return Err(VfsError::FileSystemMismatch);
        }
        let d = file.get_fs_specific_data();
        let data = &(*d)
            .as_any()
            .downcast_ref::<PipeFsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match data {
            PipeFsSpecificFileData::PipefsRoot => Ok(FileStat {
                size: 0,
                created_at: 0,
                modified_at: 0,
                permissions: permissions!(Owner:Read, Owner:Write).to_u64(),
                is_file: false,
                is_directory: true,
                is_symlink: false,
                owner_id: 0,
                group_id: 0,
                flags: FLAG_VIRTUAL | FLAG_SYSTEM,
            }),
            PipeFsSpecificFileData::PipefsWrite(id) => {
                let pipe = self.pipes.get(id).ok_or(VfsError::PathNotFound)?;
                let pguard = pipe.read();
                Ok(FileStat {
                    size: 0,
                    created_at: pguard.created_at,
                    modified_at: pguard.modified_at,
                    permissions: permissions!(Owner:Write).to_u64(),
                    is_file: true,
                    is_directory: false,
                    is_symlink: false,
                    owner_id: 0,
                    group_id: 0,
                    flags: FLAG_VIRTUAL | FLAG_SYSTEM,
                })
            }
            PipeFsSpecificFileData::PipefsRead(id) => {
                let pipe = self.pipes.get(id).ok_or(VfsError::PathNotFound)?;
                let pguard = pipe.read();
                Ok(FileStat {
                    size: pguard.readable_bytes() as u64,
                    created_at: pguard.created_at,
                    modified_at: pguard.modified_at,
                    permissions: permissions!(Owner:Read).to_u64(),
                    is_file: true,
                    is_directory: false,
                    is_symlink: false,
                    owner_id: 0,
                    group_id: 0,
                    flags: FLAG_VIRTUAL | FLAG_SYSTEM,
                })
            }
            PipeFsSpecificFileData::PipefsDir(id) => {
                let pipe = self.pipes.get(id).ok_or(VfsError::PathNotFound)?;
                let pguard = pipe.read();
                Ok(FileStat {
                    size: 0,
                    created_at: pguard.created_at,
                    modified_at: pguard.modified_at,
                    permissions: permissions!(Owner:Read, Owner:Write).to_u64(),
                    is_file: false,
                    is_directory: true,
                    is_symlink: false,
                    owner_id: 0,
                    group_id: 0,
                    flags: FLAG_VIRTUAL | FLAG_SYSTEM,
                })
            }
        }
    }

    fn create_child(
        &mut self,
        directory: &VfsFile,
        _name: &[char],
        kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError> {
        if directory.fs() != self.os_id {
            return Err(VfsError::FileSystemMismatch);
        }
        let d = directory.get_fs_specific_data();
        let data = (*d)
            .as_any()
            .downcast_ref::<PipeFsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match data {
            PipeFsSpecificFileData::PipefsRoot => {
                let id = self.next_pipe_id;
                self.next_pipe_id += 1;

                self.pipes.insert(
                    id,
                    Arc::new(RwLock::new(Box::new(Pipe::new_anonymous(64 * 1024)))),
                );

                Ok(VfsFile::new(
                    kind,
                    id.to_string().chars().collect(),
                    0,
                    self.parent_fs_os_id,
                    self.os_id,
                    Arc::new(PipeFsSpecificFileData::PipefsDir(id)),
                ))
            }
            _ => Err(VfsError::ActionNotAllowed),
        }
    }

    fn delete_file(&mut self, _file: &VfsFile) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn on_mount(
        &mut self,
        mount_point: &VfsFile,
        os_id: u64,
        root_fs: WeakArcrwb<Vfs>,
    ) -> Result<VfsFile, VfsError> {
        self.root_fs = Some(root_fs);
        self.parent_fs_os_id = mount_point.fs();
        self.mnt = Some(mount_point.clone());
        self.os_id = os_id;
        self.get_root()
    }

    fn on_pre_unmount(&mut self) -> Result<bool, VfsError> {
        Ok(true)
    }

    fn on_unmount(&mut self) -> Result<(), VfsError> {
        self.mnt = None;
        self.os_id = 0;
        self.parent_fs_os_id = 0;
        for h in self.handles.iter().copied().collect::<Vec<u64>>() {
            self.handles.dealloc_file_handle::<PipeFsHandle>(h);
        }
        Ok(())
    }

    fn get_vfs(&mut self) -> Result<WeakArcrwb<Vfs>, VfsError> {
        Ok(self
            .root_fs
            .as_ref()
            .ok_or(VfsError::FileSystemNotMounted)?
            .clone())
    }

    fn fopen(&mut self, file: &VfsFile, mode: u64) -> Result<u64, VfsError> {
        if file.fs() != self.os_id {
            return Err(VfsError::FileSystemMismatch);
        }

        let d = file.get_fs_specific_data();
        let data = &(*d)
            .as_any()
            .downcast_ref::<PipeFsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match data {
            PipeFsSpecificFileData::PipefsRead(id) => {
                if mode & OPEN_MODE_READ == 0
                    || mode & OPEN_MODE_WRITE != 0
                    || mode & OPEN_MODE_APPEND != 0
                    || mode & OPEN_MODE_CREATE != 0
                {
                    return Err(VfsError::InvalidOpenMode);
                }

                if mode & OPEN_MODE_FAIL_IF_EXISTS != 0 {
                    return Err(VfsError::FileAlreadyExists);
                }

                let pipe = self.pipes.get(id).ok_or(VfsError::PathNotFound)?;
                let mut pguard = pipe.write();
                if pguard.closed {
                    return Err(VfsError::PathNotFound);
                }
                pguard.readers += 1;
                Ok(self.handles.alloc_file_handle(PipeFsHandle {
                    pipe: pipe.clone(),
                    mode: PipeMode::Read,
                    pipe_id: *id,
                }))
            }
            PipeFsSpecificFileData::PipefsWrite(id) => {
                if mode & OPEN_MODE_READ != 0
                    || mode & OPEN_MODE_WRITE == 0
                    || mode & OPEN_MODE_APPEND != 0
                    || mode & OPEN_MODE_CREATE != 0
                {
                    return Err(VfsError::InvalidOpenMode);
                }

                if mode & OPEN_MODE_FAIL_IF_EXISTS != 0 {
                    return Err(VfsError::FileAlreadyExists);
                }

                let pipe = self.pipes.get(id).ok_or(VfsError::PathNotFound)?;
                let mut pguard = pipe.write();
                if pguard.closed {
                    return Err(VfsError::PathNotFound);
                }
                pguard.writers += 1;
                Ok(self.handles.alloc_file_handle(PipeFsHandle {
                    pipe: pipe.clone(),
                    mode: PipeMode::Write,
                    pipe_id: *id,
                }))
            }
            _ => Err(VfsError::NotFile),
        }
    }

    fn fclose(&mut self, handle: u64) -> Result<(), VfsError> {
        unsafe {
            let handle = self
                .handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            if (*handle).mode == PipeMode::Read {
                let mut wguard = (*handle).pipe.write();
                wguard.readers -= 1;
                if wguard.readers == 0 {
                    wguard.closed = true;
                    if wguard.writers == 0 {
                        self.pipes.remove(&(*handle).pipe_id);
                    }
                }
                drop(wguard);
            } else {
                let mut wguard = (*handle).pipe.write();
                wguard.writers -= 1;
                if wguard.writers == 0 {
                    wguard.closed = true;
                    if wguard.readers == 0 {
                        self.pipes.remove(&(*handle).pipe_id);
                    }
                }
                drop(wguard);
            }
        }

        if self.handles.dealloc_file_handle::<PipeFsHandle>(handle) {
            Ok(())
        } else {
            Err(VfsError::BadHandle)
        }
    }

    fn fseek(&mut self, _handle: u64, _position: SeekPosition) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fread(&mut self, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        unsafe {
            let handle = self
                .handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            if (*handle).mode == PipeMode::Read {
                let mut wguard = (*handle).pipe.write();
                if wguard.is_empty() {
                    if wguard.closed {
                        // EOF
                        return Ok(0);
                    }
                    return Err(VfsError::WouldBlock);
                }
                Ok(wguard.read(buf) as u64)
            } else {
                Err(VfsError::ActionNotAllowed)
            }
        }
    }

    fn fwrite(&mut self, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        unsafe {
            let handle = self
                .handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            if (*handle).mode == PipeMode::Write {
                let mut wguard = (*handle).pipe.write();
                if wguard.readers == 0 {
                    return Err(VfsError::BrokenPipe);
                }
                if wguard.is_full() {
                    return Err(VfsError::WouldBlock);
                }
                Ok(wguard.write(buf) as u64)
            } else {
                Err(VfsError::ActionNotAllowed)
            }
        }
    }

    fn fflush(&mut self, handle: u64) -> Result<(), VfsError> {
        unsafe {
            self.handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            Ok(())
        }
    }

    fn fsync(&mut self, handle: u64) -> Result<(), VfsError> {
        unsafe {
            self.handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            Ok(())
        }
    }

    fn fstat(&self, handle: u64) -> Result<FileStat, VfsError> {
        unsafe {
            let handle = self
                .handles
                .get_handle_data::<PipeFsHandle>(handle)
                .ok_or(VfsError::BadHandle)?;

            let pipe = (*handle).pipe.read();
            Ok(FileStat {
                size: match (*handle).mode {
                    PipeMode::Read => pipe.readable_bytes() as u64,
                    PipeMode::Write => 0,
                },
                created_at: pipe.created_at,
                modified_at: pipe.modified_at,
                permissions: match (*handle).mode {
                    PipeMode::Read => permissions!(Owner:Read).to_u64(),
                    PipeMode::Write => permissions!(Owner:Write).to_u64(),
                },
                is_file: true,
                is_directory: false,
                is_symlink: false,
                owner_id: 0,
                group_id: 0,
                flags: FLAG_VIRTUAL | FLAG_SYSTEM,
            })
        }
    }

    fn ftruncate(&mut self, _handle: u64) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }
}

pub fn init_pipefs(vfs: &mut Vfs) {
    let fs = PipeFs {
        handles: FileHandleAllocator::default(),
        mnt: None,
        os_id: 0,
        parent_fs_os_id: 0,
        pipes: BTreeMap::new(),
        root_fs: None,
        next_pipe_id: 0,
    };

    let pipes = "pipes".chars().collect::<Vec<char>>();
    vfs.mount(&pipes, Box::new(fs)).unwrap();
}
