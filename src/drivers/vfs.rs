use core::{alloc::Layout, any::Any, fmt::Debug};

use alloc::{
    alloc::{alloc, dealloc},
    boxed::Box,
    collections::{btree_map::Entry, btree_set, BTreeMap, BTreeSet},
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::RwLock;

use crate::data::either::Either;

use super::fs::virt::devfs::init_devfs;

pub type Arcrwb<T> = Arc<RwLock<Box<T>>>;
pub type WeakArcrwb<T> = Weak<RwLock<Box<T>>>;

pub fn arcrwb_new<T>(x: T) -> Arcrwb<T> {
    Arcrwb::new(RwLock::new(Box::new(x)))
}

pub fn arcrwb_new_from_box<T: ?Sized>(x: Box<T>) -> Arcrwb<T> {
    Arcrwb::new(RwLock::new(x))
}

pub fn weak_arcrwb_new<T>(x: T) -> Arcrwb<T> {
    Arcrwb::new(RwLock::new(Box::new(x)))
}

pub fn weak_arcrwb_new_from_box<T: ?Sized>(x: Box<T>) -> Arcrwb<T> {
    Arcrwb::new(RwLock::new(x))
}

#[derive(Debug)]
pub enum VfsError {
    FileSystemMismatch,
    ActionNotAllowed,
    PathNotFound,
    ReadOnly,
    FileSystemNotMounted,
    BadBufferSize,
    NotDirectory,
    NotFile,
    NotMountPoint,
    OutOfBounds,
    UnknownError,
    InvalidOpenMode,
    InvalidSeekPosition,
    BadHandle,
    AlreadyMounted,
    OutOfSpace,
    InvalidArgument,
    MaximumSizeReached,
    EntryNotFound,
    FileAlreadyExists,
    InvalidDataStructure,
    DirectoryNotEmpty,
    NameTooLong,
    ShortRead,
    Done,
    DriverError(Box<dyn core::fmt::Debug>),
}

#[derive(Clone)]
pub enum VfsFileKind {
    File,
    Directory,
    BlockDevice { device: Arcrwb<dyn BlockDevice> },
    CharacterDevice { device: Arcrwb<dyn CharacterDevice> },
    MountPoint { mounted_fs: Arcrwb<dyn FileSystem> },
}

impl Debug for VfsFileKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VfsFileKind::File => write!(f, "File"),
            VfsFileKind::Directory => write!(f, "Directory"),
            VfsFileKind::BlockDevice { .. } => write!(f, "BlockDevice"),
            VfsFileKind::CharacterDevice { .. } => write!(f, "CharacterDevice"),
            VfsFileKind::MountPoint { .. } => write!(f, "MountPoint"),
        }
    }
}

pub trait FsSpecificFileData: AsAny + Debug + Send + Sync {}

#[derive(Clone, Debug)]
pub struct VfsFile {
    kind: VfsFileKind,
    name: Vec<char>,
    size: u64,
    parent_fs: u64,
    fs: u64,
    fs_specific: Arc<dyn FsSpecificFileData>,
}

impl VfsFile {
    pub fn get_mounted_fs(&self) -> Option<Arcrwb<dyn FileSystem>> {
        match &self.kind {
            VfsFileKind::MountPoint { mounted_fs } => Some(mounted_fs.clone()),
            _ => None,
        }
    }

    pub fn get_block_device(&self) -> Option<Arcrwb<dyn BlockDevice>> {
        match &self.kind {
            VfsFileKind::BlockDevice { device } => Some(device.clone()),
            _ => None,
        }
    }

    pub fn get_char_device(&self) -> Option<Arcrwb<dyn CharacterDevice>> {
        match &self.kind {
            VfsFileKind::CharacterDevice { device } => Some(device.clone()),
            _ => None,
        }
    }

    pub fn get_fs_specific_data(&self) -> Arc<dyn FsSpecificFileData> {
        self.fs_specific.clone()
    }

    pub fn is_directory(&self) -> bool {
        matches!(self.kind, VfsFileKind::Directory)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, VfsFileKind::File)
    }

    pub fn is_block_device(&self) -> bool {
        matches!(self.kind, VfsFileKind::BlockDevice { .. })
    }

    pub fn is_char_device(&self) -> bool {
        matches!(self.kind, VfsFileKind::CharacterDevice { .. })
    }

    pub fn is_mount_point(&self) -> bool {
        matches!(self.kind, VfsFileKind::MountPoint { .. })
    }
}

impl VfsFile {
    pub const fn new(
        kind: VfsFileKind,
        name: Vec<char>,
        size: u64,
        parent_fs: u64,
        fs: u64,
        fs_specific: Arc<dyn FsSpecificFileData>,
    ) -> Self {
        Self {
            kind,
            name,
            size,
            parent_fs,
            fs,
            fs_specific,
        }
    }

    pub fn kind(&self) -> &VfsFileKind {
        &self.kind
    }

    pub fn name(&self) -> &[char] {
        &self.name
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn parent_fs(&self) -> u64 {
        self.parent_fs
    }

    pub fn fs(&self) -> u64 {
        self.fs
    }
}

pub trait BlockDevice: Send + Sync + core::fmt::Debug + AsAny {
    fn get_generation(&self) -> u64;
    fn get_block_size(&self) -> u64;
    fn get_block_count(&self) -> u64;
    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<u64, VfsError>;
    fn flush(&mut self) -> Result<(), VfsError>;
}

pub trait CharacterDevice: Send + Sync + core::fmt::Debug + AsAny {
    fn get_generation(&self) -> u64;
    fn get_size(&self) -> u64;
    fn read_chars(&self, offset: u64, buf: &mut [u8]) -> Result<u64, VfsError>;
    fn write_chars(&mut self, offset: u64, buf: &[u8]) -> Result<u64, VfsError>;
    fn flush(&mut self) -> Result<(), VfsError>;
}

pub trait AsAny {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
    fn type_name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct SubBlockDevice {
    device: Arcrwb<dyn BlockDevice>,
    generation: u64,
    begin_block: u64,
    end_block: u64,
}

impl SubBlockDevice {
    pub fn new(device: Arcrwb<dyn BlockDevice>, begin_block: u64, end_block: u64) -> Self {
        let generation = device.read().get_generation();
        Self {
            device,
            generation,
            begin_block,
            end_block,
        }
    }
}

impl BlockDevice for SubBlockDevice {
    fn get_generation(&self) -> u64 {
        self.generation
    }

    fn get_block_size(&self) -> u64 {
        self.device.read().get_block_size()
    }

    fn get_block_count(&self) -> u64 {
        self.end_block - self.begin_block
    }

    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if lba >= self.get_block_count() {
            return Err(VfsError::OutOfBounds);
        }
        self.device.read().read_block(lba, buf)
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if lba >= self.get_block_count() {
            return Err(VfsError::OutOfBounds);
        }
        let mut guard = self.device.write();
        if guard.get_generation() != self.generation {
            return Err(VfsError::ActionNotAllowed);
        }
        guard.write_block(lba, buf)
    }

    fn flush(&mut self) -> Result<(), VfsError> {
        self.device.write().flush()
    }
}

#[derive(Debug, Clone)]
pub struct BlockDeviceAsCharacterDevice {
    device: Arcrwb<dyn BlockDevice>,
}

impl BlockDeviceAsCharacterDevice {
    pub fn new(device: Arcrwb<dyn BlockDevice>) -> Self {
        Self { device }
    }
}

impl CharacterDevice for BlockDeviceAsCharacterDevice {
    fn get_generation(&self) -> u64 {
        self.device.read().get_generation()
    }

    fn get_size(&self) -> u64 {
        let guard = self.device.read();
        guard.get_block_count() * guard.get_block_size()
    }

    fn flush(&mut self) -> Result<(), VfsError> {
        self.device.write().flush()
    }

    fn read_chars(&self, mut offset: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        let to_read = (buf.len() as u64).min(self.get_size() - offset) as usize;
        let mut read: usize = 0;

        let guard = self.device.read();

        let block_size = guard.get_block_size() as usize;
        let mut block = alloc::vec![0u8; block_size];

        while read < to_read {
            let lba = offset / guard.get_block_size();
            let pos = (offset % guard.get_block_size()) as usize;

            let rem_read = to_read - read;
            let max_read_sector = block_size - pos;
            let to_read_sector = rem_read.min(max_read_sector);

            guard.read_block(lba, &mut block)?;
            buf[read..read + to_read_sector].copy_from_slice(&block[pos..pos + to_read_sector]);

            read += to_read_sector;
            offset += to_read_sector as u64;
        }
        Ok(read as u64)
    }

    fn write_chars(&mut self, mut offset: u64, buf: &[u8]) -> Result<u64, VfsError> {
        let to_write = (buf.len() as u64).min(self.get_size() - offset) as usize;
        let mut write: usize = 0;

        let mut guard = self.device.write();

        let block_size = guard.get_block_size() as usize;
        let mut block = alloc::vec![0u8; block_size];

        while write < to_write {
            let lba = offset / guard.get_block_size();
            let pos = (offset % guard.get_block_size()) as usize;

            let rem_write = to_write - write;
            let max_write_sector = block_size - pos;
            let to_write_sector = rem_write.min(max_write_sector);

            if to_write_sector != block_size {
                guard.read_block(lba, &mut block)?;
                block[pos..pos + to_write_sector]
                    .copy_from_slice(&buf[write..write + to_write_sector]);
                guard.write_block(lba, &block)?;
            } else {
                guard.write_block(lba, &buf[write..write + to_write_sector])?;
            }

            write += to_write_sector;
            offset += to_write_sector as u64;
        }
        Ok(write as u64)
    }
}

pub const OPEN_MODE_READ: u64 = 1 << 0;
pub const OPEN_MODE_WRITE: u64 = 1 << 1;
pub const OPEN_MODE_APPEND: u64 = 1 << 2;
pub const OPEN_MODE_NO_RESIZE: u64 = 1 << 3;
pub const OPEN_MODE_CREATE: u64 = 1 << 4;
pub const OPEN_MODE_FAIL_IF_EXISTS: u64 = 1 << 5;

#[derive(Debug, Clone, Copy)]
pub enum SeekPosition {
    FromStart(u64),
    FromCurrent(i64),
    FromEnd(u64),
}

pub const FLAG_VIRTUAL: u64 = 1 << 0;
pub const FLAG_READ_ONLY: u64 = 1 << 1;
pub const FLAG_HIDDEN: u64 = 1 << 2;
pub const FLAG_SYSTEM: u64 = 1 << 3;
pub const FLAG_PHYSICAL_BLOCK_DEVICE: u64 = 1 << 4;
pub const FLAG_VIRTUAL_BLOCK_DEVICE: u64 = 1 << 5;
pub const FLAG_PHYSICAL_CHARACTER_DEVICE: u64 = 1 << 6;
pub const FLAG_VIRTUAL_CHARACTER_DEVICE: u64 = 1 << 7;
pub const FLAG_PARTITIONED_DEVICE: u64 = 1 << 8;

#[derive(Debug)]
pub struct FileStat {
    pub size: u64,
    pub created_at: u64,
    pub modified_at: u64,
    pub permissions: u64,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub owner_id: u64,
    pub group_id: u64,
    pub flags: u64,
}

pub trait FileSystem: Send + Sync + core::fmt::Debug + AsAny {
    /// Returns this file system's ID
    fn os_id(&mut self) -> u64;

    /// Returns the file system type
    fn fs_type(&mut self) -> String;

    /// Flushes, and forces the file system to write all pending writes to disk
    fn fs_flush(&mut self) -> Result<(), VfsError>;

    /// Returns the block device used by the file system, None is applicable only to in-memory file systems
    fn host_block_device(&mut self) -> Option<Arcrwb<dyn BlockDevice>>;

    /// Returns the root of the file system
    fn get_root(&mut self) -> Result<VfsFile, VfsError>;

    /// Returns the mount point of the file system, none for the absolute root
    fn get_mount_point(&mut self) -> Result<Option<VfsFile>, VfsError>;

    /// Finds a child of the given file
    fn get_child(&mut self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError>;

    /// Lists the children of the given file if it is a directory
    fn list_children(&mut self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError>;

    /// Returns the file at the given path, from this file system's root
    fn get_file(&mut self, path: &[char]) -> Result<VfsFile, VfsError>;

    /// Returns the stats of the given file
    fn get_stats(&mut self, file: &VfsFile) -> Result<FileStat, VfsError>;

    /// Creates a child file at the given path
    fn create_child(
        &mut self,
        directory: &VfsFile,
        name: &[char],
        kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError>;

    /// Deletes a file, or an empty directory
    fn delete_file(&mut self, file: &VfsFile) -> Result<(), VfsError>;

    /// Called when filesystem is mounted
    /// Returns the root directory of the mounted filesystem
    fn on_mount(
        &mut self,
        mount_point: &VfsFile,
        os_id: u64,
        root_fs: WeakArcrwb<Vfs>,
    ) -> Result<VfsFile, VfsError>;

    /// Called when filesystem should be unmounted
    /// Should return true when filesystem is ready to be unmounted false if it needs to do something else
    fn on_pre_unmount(&mut self) -> Result<bool, VfsError>;

    /// Called when filesystem is unmounted
    fn on_unmount(&mut self) -> Result<(), VfsError>;

    /// Gets the root file system
    fn get_vfs(&mut self) -> Result<WeakArcrwb<Vfs>, VfsError>;

    /// Opens a file
    /// Returns the file handle
    fn fopen(&mut self, file: &VfsFile, mode: u64) -> Result<u64, VfsError>;

    /// Closes a file
    fn fclose(&mut self, handle: u64) -> Result<(), VfsError>;

    /// Seeks a file
    /// Returns the new position
    fn fseek(&mut self, handle: u64, position: SeekPosition) -> Result<u64, VfsError>;

    /// Reads from a file
    /// Returns the number of bytes read
    fn fread(&mut self, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError>;

    /// Writes to a file
    /// Returns the number of bytes written
    fn fwrite(&mut self, handle: u64, buf: &[u8]) -> Result<u64, VfsError>;

    /// Flushes a file
    fn fflush(&mut self, handle: u64) -> Result<(), VfsError>;

    /// Synchronizes a file
    fn fsync(&mut self, handle: u64) -> Result<(), VfsError>;

    /// Gets stats of a file
    fn fstat(&self, handle: u64) -> Result<FileStat, VfsError>;

    /// Truncates a file
    /// Returns the new size
    fn ftruncate(&mut self, handle: u64) -> Result<u64, VfsError>;
}

pub struct PathSplitter<'a> {
    path: &'a [char],
    idx: usize,
    last_part: Option<&'a [char]>,
}

pub struct PathSplitterPeek<'a, 'b>
where
    'a: 'b,
{
    splitter: &'b mut PathSplitter<'a>,
    slice: &'a [char],
    idx: usize,
}

impl<'a> PathSplitterPeek<'a, '_> {
    pub fn apply(self) -> &'a [char] {
        self.splitter.last_part = Some(self.slice);
        self.splitter.idx = self.idx;
        self.slice
    }

    pub fn get_path_part(&self) -> &'a [char] {
        self.slice
    }
}

impl<'a> PathSplitter<'a> {
    pub fn new(path: &'a [char]) -> Self {
        let mut idx = 0;
        while idx < path.len() && path[idx] == '/' {
            idx += 1;
        }
        Self {
            path,
            idx,
            last_part: None,
        }
    }

    pub fn is_done(&self) -> bool {
        self.idx >= self.path.len()
    }

    pub fn peek<'b>(&'b mut self) -> Option<PathSplitterPeek<'a, 'b>>
    where
        'a: 'b,
    {
        if self.is_done() {
            None
        } else {
            let mut idx = self.idx;
            while idx < self.path.len() && self.path[idx] != '/' {
                idx += 1;
            }
            let slice = &self.path[self.idx..idx];
            while idx < self.path.len() && self.path[idx] == '/' {
                idx += 1;
            }

            Some(PathSplitterPeek {
                splitter: self,
                slice,
                idx,
            })
        }
    }

    pub fn next_part(&mut self) -> &'a [char] {
        match self.peek() {
            None => &self.path[self.idx..],
            Some(peek) => peek.apply(),
        }
    }

    pub fn last_part(&self) -> Option<&[char]> {
        self.last_part
    }
}

pub struct PathTraverse<'a, 'b> {
    spliter: PathSplitter<'a>,
    fs: Either<Arcrwb<dyn FileSystem>, &'b mut dyn FileSystem>,
    curr: VfsFile,
}

impl<'a, 'b> PathTraverse<'a, 'b> {
    pub fn new(
        path: &'a [char],
        fs: Arcrwb<dyn FileSystem>,
    ) -> Result<PathTraverse<'a, 'b>, VfsError> {
        Ok(PathTraverse {
            spliter: PathSplitter::new(path),
            curr: fs.write().get_root()?,
            fs: Either::new_left(fs.clone()),
        })
    }

    pub fn new_owned(
        path: &'a [char],
        fs: &'b mut dyn FileSystem,
    ) -> Result<PathTraverse<'a, 'b>, VfsError> {
        Ok(PathTraverse {
            spliter: PathSplitter::new(path),
            curr: fs.get_root()?,
            fs: Either::new_right(fs),
        })
    }

    pub fn is_done(&self) -> bool {
        self.spliter.is_done()
    }

    pub fn find_next(&mut self) -> Result<VfsFile, VfsError> {
        if self.is_done() {
            return Err(VfsError::Done);
        }
        if let Some(fs) = self.curr.get_mounted_fs() {
            {
                let mut guard = fs.write();
                self.curr = guard.get_root()?;
            }
            self.fs = Either::new_left(fs.clone());
        }

        let Some(peek) = self.spliter.peek() else {
            return Err(VfsError::Done);
        };
        let part = peek.slice;

        let next = self.fs.referenced_mut().convert(
            |fs| fs.write().get_child(&self.curr, part),
            |fs| fs.get_child(&self.curr, part),
        )?;

        peek.apply();

        self.curr = next.clone();
        Ok(next)
    }

    pub fn mkdir(&mut self) -> Result<VfsFile, VfsError> {
        if self.is_done() {
            return Err(VfsError::Done);
        }

        let Some(peek) = self.spliter.peek() else {
            return Err(VfsError::Done);
        };
        let part = peek.slice;

        let next = self.fs.referenced_mut().convert(
            |fs| {
                fs.write()
                    .create_child(&self.curr, part, VfsFileKind::Directory)
            },
            |fs| fs.create_child(&self.curr, part, VfsFileKind::Directory),
        )?;

        peek.apply();

        self.curr = next.clone();
        Ok(next)
    }

    pub fn extract_splitter(self) -> PathSplitter<'a> {
        self.spliter
    }

    pub fn get_splitter(&self) -> &PathSplitter<'a> {
        &self.spliter
    }
}

#[derive(Debug)]
pub struct MountNode {
    children: BTreeMap<Vec<char>, MountNode>,
    contents: Option<WeakArcrwb<dyn FileSystem>>,
}

#[derive(Debug)]
pub struct MountingPointsManager {
    tree: MountNode,
}

impl Default for MountingPointsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MountingPointsManager {
    pub fn new() -> Self {
        Self {
            tree: MountNode {
                children: BTreeMap::new(),
                contents: None,
            },
        }
    }

    pub fn register_fs(
        &mut self,
        name: &[char],
        fs: Arcrwb<dyn FileSystem>,
    ) -> Result<(), VfsError> {
        let mut splitter = PathSplitter::new(name);

        let mut node = &mut self.tree;
        while !splitter.is_done() {
            if node.contents.is_some() {
                return Err(VfsError::AlreadyMounted);
            }
            let part = splitter.next_part();
            match node.children.entry(part.to_vec()) {
                Entry::Vacant(entry) => {
                    node = entry.insert(MountNode {
                        children: BTreeMap::new(),
                        contents: None,
                    });
                }
                Entry::Occupied(entry) => {
                    node = entry.into_mut();
                }
            }
        }

        if !node.children.is_empty() || node.contents.is_some() {
            return Err(VfsError::AlreadyMounted);
        }

        node.contents = Some(Arc::downgrade(&fs));
        Ok(())
    }

    pub fn search_fs<'a>(
        &self,
        name: &'a [char],
    ) -> Option<(WeakArcrwb<dyn FileSystem>, PathSplitter<'a>)> {
        let mut splitter = PathSplitter::new(name);

        let mut node = &self.tree;
        while !splitter.is_done() {
            let part = splitter.next_part();
            match node.children.get(part) {
                Some(child) => {
                    node = child;
                }
                None => {
                    return node.contents.as_ref().map(|fs| (fs.clone(), splitter));
                }
            }
        }

        node.contents.as_ref().map(|fs| (fs.clone(), splitter))
    }

    pub fn remove_fs(&mut self, name: &[char]) -> Result<WeakArcrwb<dyn FileSystem>, VfsError> {
        Self::remove_fs_recursive(&mut self.tree, PathSplitter::new(name))
    }

    fn remove_fs_recursive(
        node: &mut MountNode,
        mut splitter: PathSplitter,
    ) -> Result<WeakArcrwb<dyn FileSystem>, VfsError> {
        if splitter.is_done() {
            match node.contents.take() {
                Some(fs) => Ok(fs),
                None => Err(VfsError::PathNotFound),
            }
        } else {
            let part = splitter.next_part();
            match node.children.get_mut(part) {
                Some(child) => {
                    let fs = Self::remove_fs_recursive(child, splitter)?;

                    if child.children.is_empty() && child.contents.is_none() {
                        node.children.remove(part);
                    }

                    Ok(fs)
                }
                None => Err(VfsError::PathNotFound),
            }
        }
    }
}

#[derive(Debug)]
pub struct Vfs {
    fs_by_id: Arcrwb<BTreeMap<u64, Arcrwb<dyn FileSystem>>>,

    mounting_points_manager: MountingPointsManager,

    root_fs: Option<WeakArcrwb<Vfs>>,
    os_id_count: u64,
}

impl Vfs {
    pub fn next_os_id(&mut self) -> u64 {
        self.os_id_count += 1;
        self.os_id_count
    }

    pub fn get_fs_by_id(&self, id: u64) -> Option<Arcrwb<dyn FileSystem>> {
        self.fs_by_id.read().get(&id).cloned()
    }

    fn register_fs(
        &mut self,
        os_id: u64,
        name: &[char],
        ptr: &Arcrwb<dyn FileSystem>,
    ) -> Result<(), VfsError> {
        let mut wguard = self.fs_by_id.write();
        wguard.insert(os_id, ptr.clone());

        self.mounting_points_manager.register_fs(name, ptr.clone())
    }

    pub fn mount(&mut self, name: &[char], fs: Box<dyn FileSystem>) -> Result<VfsFile, VfsError> {
        let root_fs = self.root_fs.clone().ok_or(VfsError::FileSystemNotMounted)?;
        let name = name.to_vec();

        let os_id = self.next_os_id();
        let ptr = arcrwb_new_from_box(fs);

        self.register_fs(os_id, &name, &ptr)?;

        let mount_point = VfsFile {
            kind: VfsFileKind::MountPoint {
                mounted_fs: ptr.clone(),
            },
            name,
            size: 0,
            parent_fs: self.os_id(),
            fs: os_id,
            fs_specific: Arc::new(VfsSpecificFileData),
        };

        (&mut **ptr.write() as &mut dyn FileSystem).on_mount(&mount_point, os_id, root_fs)?;

        Ok(mount_point)
    }

    pub fn unmount(&mut self, name: &[char]) -> Result<(), VfsError> {
        let fs = self.mounting_points_manager.remove_fs(name)?;
        let Some(fs) = fs.upgrade() else {
            return Err(VfsError::UnknownError);
        };
        let mut guard = fs.write();

        let id = guard.os_id();

        #[allow(clippy::never_loop)]
        while !guard.on_pre_unmount()? {
            // TODO: Let driver do their thing
            break;
        }
        guard.on_unmount()?;

        {
            let mut wguard = self.fs_by_id.write();
            wguard.remove(&id);
        }

        Ok(())
    }

    pub fn get_stats(&mut self, path: &[char]) -> Result<Option<FileStat>, VfsError> {
        match self.get_file(path) {
            Ok(file) => match file.get_mounted_fs() {
                Some(fs) => {
                    let mut guard = fs.write();
                    let root = guard.get_root()?;
                    guard.get_stats(&root).map(Some)
                }
                None => {
                    let fs = self
                        .get_fs_by_id(file.fs)
                        .ok_or(VfsError::FileSystemNotMounted)?;

                    let mut guard = fs.write();
                    guard.get_stats(&file).map(Some)
                }
            },
            Err(VfsError::PathNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Default)]
pub struct VfsSpecificFileData;

impl FsSpecificFileData for VfsSpecificFileData {}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn type_name(&self) -> &'static str {
        core::any::type_name::<T>()
    }
}

macro_rules! default_get_file_implementation {
    () => {
        fn get_file(&mut self, path: &[char]) -> Result<VfsFile, VfsError> {
            let mut traverse = $crate::drivers::vfs::PathTraverse::new_owned(path, self)?;
            if traverse.is_done() {
                return self.get_root();
            }
            loop {
                let result = traverse.find_next()?;
                if traverse.is_done() {
                    break Ok(result);
                }
            }
        }
    };
}
pub(crate) use default_get_file_implementation;

impl FileSystem for Vfs {
    fn os_id(&mut self) -> u64 {
        1
    }

    fn fs_type(&mut self) -> String {
        "vfs".to_string()
    }

    fn fs_flush(&mut self) -> Result<(), VfsError> {
        Ok(())
    }

    fn host_block_device(&mut self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
    }

    fn get_root(&mut self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile {
            kind: VfsFileKind::Directory,
            name: "/".chars().collect(),
            size: 0,
            parent_fs: self.os_id(),
            fs: self.os_id(),
            fs_specific: Arc::new(VfsSpecificFileData),
        })
    }

    fn get_mount_point(&mut self) -> Result<Option<VfsFile>, VfsError> {
        Ok(None)
    }

    fn get_child(&mut self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.is_mount_point() {
            return file
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?
                .write()
                .get_child(file, child);
        }

        let mut node = &self.mounting_points_manager.tree;
        let mut splitter = PathSplitter::new(file.name());
        while !splitter.is_done() {
            let part = splitter.next_part();
            match node.children.get(part) {
                None => return Err(VfsError::PathNotFound),
                Some(child) => node = child,
            }
        }

        match node.children.get(child) {
            None => Err(VfsError::PathNotFound),
            Some(c) => match &c.contents {
                None => Ok(VfsFile {
                    kind: VfsFileKind::Directory,
                    name: [file.name(), &['/'], child].concat(),
                    size: 0,
                    parent_fs: self.os_id(),
                    fs: self.os_id(),
                    fs_specific: Arc::new(VfsSpecificFileData),
                }),
                Some(fs) => {
                    let fs = fs.upgrade().ok_or(VfsError::UnknownError)?;
                    let mut guard = fs.write();
                    Ok(VfsFile {
                        kind: VfsFileKind::MountPoint {
                            mounted_fs: fs.clone(),
                        },
                        name: [file.name(), &['/'], child].concat(),
                        size: 0,
                        parent_fs: self.os_id(),
                        fs: guard.os_id(),
                        fs_specific: Arc::new(VfsSpecificFileData),
                    })
                }
            },
        }
    }

    fn list_children(&mut self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.is_mount_point() {
            let fs = file
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?;
            let mut guard = fs.write();

            let root = guard.get_root()?;
            return guard.list_children(&root);
        }
        if !file.is_directory() {
            return Err(VfsError::NotDirectory);
        }
        if file.fs != self.os_id() {
            let fs = self
                .get_fs_by_id(file.fs)
                .ok_or(VfsError::FileSystemNotMounted)?;
            return fs.write().list_children(file);
        }
        let os_id = self.os_id();

        let mut node = &self.mounting_points_manager.tree;
        let mut splitter = PathSplitter::new(file.name());
        while !splitter.is_done() {
            let part = splitter.next_part();
            match node.children.get(part) {
                None => return Err(VfsError::PathNotFound),
                Some(child) => node = child,
            }
        }

        Ok(node
            .children
            .iter()
            .filter_map(|(k, node)| match &node.contents {
                None => Some(VfsFile {
                    kind: VfsFileKind::Directory,
                    name: [file.name(), &['/'], k].concat(),
                    size: 0,
                    parent_fs: os_id,
                    fs: os_id,
                    fs_specific: Arc::new(VfsSpecificFileData),
                }),
                Some(fs) => {
                    let fs = fs.upgrade()?;
                    let os_id = fs.write().os_id();
                    Some(VfsFile {
                        kind: VfsFileKind::MountPoint {
                            mounted_fs: fs.clone(),
                        },
                        name: k.to_vec(),
                        size: 0,
                        parent_fs: os_id,
                        fs: os_id,
                        fs_specific: Arc::new(VfsSpecificFileData),
                    })
                }
            })
            .collect::<Vec<_>>())
    }

    default_get_file_implementation!();

    fn get_stats(&mut self, _file: &VfsFile) -> Result<FileStat, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn create_child(
        &mut self,
        directory: &VfsFile,
        _name: &[char],
        _kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError> {
        if directory.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if !directory.is_directory() {
            return Err(VfsError::NotDirectory);
        }
        Err(VfsError::ActionNotAllowed)
    }

    fn delete_file(&mut self, _file: &VfsFile) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn on_mount(
        &mut self,
        _mount_point: &VfsFile,
        _os_id: u64,
        _root_fs: WeakArcrwb<Vfs>,
    ) -> Result<VfsFile, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn on_pre_unmount(&mut self) -> Result<bool, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn on_unmount(&mut self) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn get_vfs(&mut self) -> Result<WeakArcrwb<Vfs>, VfsError> {
        Ok(self
            .root_fs
            .as_ref()
            .ok_or(VfsError::FileSystemNotMounted)?
            .clone())
    }

    fn fopen(&mut self, file: &VfsFile, _mode: u64) -> Result<u64, VfsError> {
        // Vfs only contains the root directory, and mount points which can't be opened
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        Err(VfsError::ActionNotAllowed)
    }

    fn fclose(&mut self, _file: u64) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fflush(&mut self, _handle: u64) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fread(&mut self, _handle: u64, _buf: &mut [u8]) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fwrite(&mut self, _handle: u64, _buf: &[u8]) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fstat(&self, _handle: u64) -> Result<FileStat, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fseek(&mut self, _handle: u64, _position: SeekPosition) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fsync(&mut self, _handle: u64) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn ftruncate(&mut self, _handle: u64) -> Result<u64, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct VfsHandleData<T: Sized + Clone + Debug> {
    layout: Layout,
    data: T,
}

#[derive(Debug, Default)]
pub struct FileHandleAllocator {
    handles: BTreeSet<u64>,
}

impl FileHandleAllocator {
    pub fn alloc_file_handle<T: Sized + Clone + Debug>(&mut self, data: T) -> u64 {
        let handle = unsafe {
            let layout = Layout::from_size_align_unchecked(
                size_of::<VfsHandleData<T>>(),
                align_of::<VfsHandleData<T>>(),
            );
            let handle = alloc(layout) as *mut VfsHandleData<T>;
            handle.write(VfsHandleData { data, layout });
            handle as u64
        };
        self.handles.insert(handle);
        handle
    }

    /// # Safety
    /// Caller must ensure that the handle is valid and was allocated using `alloc_file_handle` with the same `T` type
    pub unsafe fn get_handle_data<T: Sized + Clone + Debug>(&self, handle: u64) -> Option<*mut T> {
        if !self.handles.contains(&handle) {
            return None;
        }
        let handle_data = handle as *mut VfsHandleData<T>;
        Some(&mut (*handle_data).data as *mut T)
    }

    pub fn dealloc_file_handle<T: Sized + Clone + Debug>(&mut self, handle: u64) {
        if self.handles.contains(&handle) {
            unsafe {
                dealloc(
                    handle as *mut u8,
                    (*(handle as *mut VfsHandleData<T>)).layout,
                )
            };
            self.handles.remove(&handle);
        }
    }

    pub fn iter(&self) -> btree_set::Iter<u64> {
        self.handles.iter()
    }
}

static mut VFS: Option<Arcrwb<Vfs>> = None;

pub fn get_vfs() -> Arcrwb<Vfs> {
    unsafe {
        match VFS {
            Some(ref v) => v.clone(),
            None => {
                let v = Vfs {
                    fs_by_id: arcrwb_new(BTreeMap::new()),
                    mounting_points_manager: MountingPointsManager::new(),
                    root_fs: None,
                    os_id_count: 1,
                };
                VFS = Some(arcrwb_new(v));
                #[allow(static_mut_refs)]
                let ptr = VFS.clone().unwrap();
                let iptr = Some(Arc::downgrade(&ptr.clone()));
                let mut wguard = ptr.write();
                wguard.root_fs = iptr;

                init_vfs(&mut wguard);

                #[allow(static_mut_refs)]
                VFS.clone().unwrap()
            }
        }
    }
}

fn init_vfs(vfs: &mut Vfs) {
    init_devfs(vfs);
}
