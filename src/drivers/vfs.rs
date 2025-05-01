use core::any::Any;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
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
    FileSystemNotMounted,
    BadBufferSize,
    NotDirectory,
    NotMountPoint,
    DriverError(Box<dyn core::fmt::Debug>),
}

#[derive(Debug, Clone)]
pub enum VfsFileKind {
    File,
    Directory,
    BlockDevice { device: Arcrwb<dyn BlockDevice> },
    MountPoint { mounted_fs: Arcrwb<dyn FileSystem> },
}

#[derive(Clone, Debug)]
pub struct VfsFile {
    kind: VfsFileKind,
    name: Vec<char>,
    size: u64,
    parent_fs: u64,
    fs: u64,
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

    pub fn is_directory(&self) -> bool {
        matches!(self.kind, VfsFileKind::Directory)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, VfsFileKind::File)
    }

    pub fn is_block_device(&self) -> bool {
        matches!(self.kind, VfsFileKind::BlockDevice { .. })
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
    ) -> Self {
        Self {
            kind,
            name,
            size,
            parent_fs,
            fs,
        }
    }

    pub fn kind(&self) -> &VfsFileKind {
        &self.kind
    }

    pub fn name(&self) -> &Vec<char> {
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

pub trait BlockDevice: Send + Sync + core::fmt::Debug {
    fn get_block_size(&self) -> u64;
    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<u64, VfsError>;
}

pub trait AsAny {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
}

pub trait FileSystem: Send + Sync + core::fmt::Debug + AsAny {
    /// Returns this file system's ID
    fn os_id(&self) -> u64;

    /// Returns the file system type
    fn fs_type(&self) -> String;

    /// Returns the block device used by the file system, None is applicable only to in-memory file systems
    fn host_block_device(&self) -> Option<Arcrwb<dyn BlockDevice>>;

    /// Returns the root of the file system
    fn get_root(&self) -> VfsFile;

    /// Returns the mount point of the file system, none for the absolute root
    fn get_mount_point(&self) -> Result<Option<VfsFile>, VfsError>;

    /// Finds a child of the given file
    fn get_child(&self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError>;

    /// Lists the children of the given file if it is a directory
    fn list_children(&self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError>;

    /// Returns the file at the given path, from this file system's root
    fn get_file(&self, path: &[char]) -> Result<VfsFile, VfsError>;

    /// Creates a child file at the given path
    fn create_child(
        &mut self,
        directory: &VfsFile,
        name: &[char],
        kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError>;

    /// Mounts a file system in the given directory
    fn mount(
        &mut self,
        directory: &VfsFile,
        name: &[char],
        fs: Box<dyn FileSystem>,
    ) -> Result<VfsFile, VfsError>;

    /// Unmounts a file system at the given mount point
    fn unmount(&mut self, mount_point: &VfsFile) -> Result<(), VfsError>;

    /// Returns all sub file systems
    fn sub_file_systems(&self) -> Result<Vec<Arcrwb<dyn FileSystem>>, VfsError>;

    /// Called when filesystem is mounted
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
    fn get_vfs(&self) -> Result<WeakArcrwb<Vfs>, VfsError>;
}

pub struct PathSplitter<'a> {
    path: &'a [char],
    idx: usize,
    last_part: Option<&'a [char]>,
}

impl<'a> PathSplitter<'a> {
    pub fn new(path: &'a [char]) -> Self {
        let mut idx = 0;
        while path[idx] == '/' {
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

    pub fn next_part(&mut self) -> &[char] {
        let mut idx = self.idx;
        while idx < self.path.len() && self.path[idx] != '/' {
            idx += 1;
        }
        let slice = &self.path[self.idx..idx];
        self.last_part = Some(slice);
        self.idx = idx + 1;
        slice
    }

    pub fn last_part(&self) -> Option<&[char]> {
        self.last_part
    }
}

pub struct PathTraverse<'a, 'b> {
    spliter: PathSplitter<'a>,
    fs: Either<Arcrwb<dyn FileSystem>, &'b dyn FileSystem>,
    curr: VfsFile,
}

impl<'a, 'b> PathTraverse<'a, 'b> {
    pub fn new(
        path: &'a [char],
        fs: Arcrwb<dyn FileSystem>,
    ) -> Result<PathTraverse<'a, 'b>, VfsError> {
        Ok(PathTraverse {
            spliter: PathSplitter::new(path),
            curr: fs.read().get_root(),
            fs: Either::new_left(fs.clone()),
        })
    }

    pub fn new_owned(
        path: &'a [char],
        fs: &'b dyn FileSystem,
    ) -> Result<PathTraverse<'a, 'b>, VfsError> {
        Ok(PathTraverse {
            spliter: PathSplitter::new(path),
            curr: fs.get_root(),
            fs: Either::new_right(fs),
        })
    }

    pub fn is_done(&self) -> bool {
        self.spliter.is_done()
    }

    pub fn find_next(&mut self) -> Result<VfsFile, VfsError> {
        if self.is_done() {
            return Err(VfsError::PathNotFound);
        }
        if let Some(fs) = self.curr.get_mounted_fs() {
            {
                let guard = fs.read();
                self.curr = guard.get_root();
            }
            self.fs = Either::new_left(fs.clone());
        }

        let part = self.spliter.next_part();
        let next = self.fs.referenced().convert(
            |fs| fs.read().get_child(&self.curr, part),
            |fs| fs.get_child(&self.curr, part),
        )?;

        self.curr = next.clone();
        Ok(next)
    }

    pub fn extract_splitter(self) -> PathSplitter<'a> {
        self.spliter
    }
}

#[derive(Debug)]
pub struct Vfs {
    fs_by_id: Arcrwb<BTreeMap<u64, Arcrwb<dyn FileSystem>>>,
    fs_by_name: Arcrwb<BTreeMap<Vec<char>, Arcrwb<dyn FileSystem>>>,

    root_fs: Option<WeakArcrwb<Vfs>>,
    os_id_count: u64,
}

impl Vfs {
    pub fn next_os_id(&mut self) -> u64 {
        self.os_id_count += 1;
        self.os_id_count
    }
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl FileSystem for Vfs {
    fn os_id(&self) -> u64 {
        1
    }

    fn fs_type(&self) -> String {
        "vfs".to_string()
    }

    fn host_block_device(&self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
    }

    fn get_root(&self) -> VfsFile {
        VfsFile {
            kind: VfsFileKind::Directory,
            name: "/".chars().collect(),
            size: 0,
            parent_fs: self.os_id(),
            fs: self.os_id(),
        }
    }

    fn get_mount_point(&self) -> Result<Option<VfsFile>, VfsError> {
        Ok(None)
    }

    fn get_child(&self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name != ['/'] && !file.is_mount_point() {
            return Err(VfsError::PathNotFound);
        }
        if file.is_mount_point() {
            return file
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?
                .read()
                .get_child(file, child);
        }
        let fs = self
            .fs_by_name
            .read()
            .get(child)
            .cloned()
            .ok_or(VfsError::PathNotFound)?;

        let fs_id = fs.read().os_id();

        Ok(VfsFile {
            kind: VfsFileKind::MountPoint {
                mounted_fs: fs.clone(),
            },
            name: child.to_vec(),
            size: 0,
            parent_fs: self.os_id(),
            fs: fs_id,
        })
    }

    fn list_children(&self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name != ['/'] && !file.is_mount_point() {
            return Err(VfsError::PathNotFound);
        }
        if file.is_mount_point() {
            let fs = file
                .get_mounted_fs()
                .ok_or(VfsError::FileSystemNotMounted)?;
            let guard = fs.read();

            let root = guard.get_root();
            return guard.list_children(&root);
        }

        Ok(self
            .fs_by_name
            .read()
            .iter()
            .map(|(k, fs)| {
                let _ = 0;
                VfsFile {
                    kind: VfsFileKind::MountPoint {
                        mounted_fs: fs.clone(),
                    },
                    name: k.to_vec(),
                    size: 0,
                    parent_fs: self.os_id(),
                    fs: fs.read().os_id(),
                }
            })
            .collect::<Vec<_>>())
    }

    fn get_file(&self, path: &[char]) -> Result<VfsFile, VfsError> {
        let mut traverse = PathTraverse::new_owned(path, self)?;
        loop {
            let result = traverse.find_next()?;
            if traverse.is_done() {
                break Ok(result);
            }
        }
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

    fn mount(
        &mut self,
        directory: &VfsFile,
        name: &[char],
        fs: Box<dyn FileSystem>,
    ) -> Result<VfsFile, VfsError> {
        if directory.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if !directory.is_directory() {
            return Err(VfsError::NotDirectory);
        }
        if directory.name != ['/'] {
            return Err(VfsError::ActionNotAllowed);
        }
        let root_fs = self.root_fs.clone().ok_or(VfsError::FileSystemNotMounted)?;

        let name = name.to_vec();

        let os_id = self.next_os_id();
        let ptr = arcrwb_new_from_box(fs);

        {
            let mut wguard = self.fs_by_id.write();
            wguard.insert(os_id, ptr.clone());

            let mut wguard = self.fs_by_name.write();
            wguard.insert(name.to_vec(), ptr.clone());
        }

        let mount_point = VfsFile {
            kind: VfsFileKind::MountPoint {
                mounted_fs: ptr.clone(),
            },
            name: name.to_vec(),
            size: 0,
            parent_fs: self.os_id(),
            fs: os_id,
        };

        (&mut **ptr.write() as &mut dyn FileSystem).on_mount(&mount_point, os_id, root_fs)?;

        Ok(mount_point)
    }

    fn unmount(&mut self, mount_point: &VfsFile) -> Result<(), VfsError> {
        if mount_point.fs != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if !mount_point.is_mount_point() {
            return Err(VfsError::NotMountPoint);
        }

        {
            let mut wguard = self.fs_by_id.write();
            wguard.remove(&mount_point.fs);

            let mut wguard = self.fs_by_name.write();
            wguard.remove(&mount_point.name);
        }

        Ok(())
    }

    fn sub_file_systems(&self) -> Result<Vec<Arcrwb<dyn FileSystem>>, VfsError> {
        let v = self.fs_by_id.read().values().cloned().collect::<Vec<_>>();
        Ok(v)
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

    fn get_vfs(&self) -> Result<WeakArcrwb<Vfs>, VfsError> {
        Ok(self
            .root_fs
            .as_ref()
            .ok_or(VfsError::FileSystemNotMounted)?
            .clone())
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
                    fs_by_name: arcrwb_new(BTreeMap::new()),
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
