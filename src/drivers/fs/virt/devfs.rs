use core::{alloc::Layout, fmt::Debug};

use alloc::{
    alloc::{alloc, dealloc},
    boxed::Box,
    collections::{btree_map::Entry, BTreeMap},
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::rwlock::RwLock;

use crate::drivers::{
    disk::init_disk_drivers,
    pci::{self, PciDevice},
    vfs::{
        Arcrwb, AsAny, BlockDevice, FileStat, FileSystem, PathTraverse, SeekPosition, Vfs,
        VfsError, VfsFile, VfsFileKind, WeakArcrwb,
    },
    vga::init_vga,
};

pub const fn fseek_helper(seek: SeekPosition, current_position: u64, len: u64) -> Option<u64> {
    match seek {
        SeekPosition::FromStart(position) => {
            if position > len {
                None
            } else {
                Some(position)
            }
        }
        SeekPosition::FromCurrent(position) => {
            if position < 0 {
                let abs_pos = position.unsigned_abs();
                if abs_pos > current_position {
                    None
                } else {
                    Some(current_position - abs_pos)
                }
            } else {
                let (new_pos, overflow) = current_position.overflowing_add(position as u64);
                if overflow || new_pos > len {
                    None
                } else {
                    Some(new_pos)
                }
            }
        }
        SeekPosition::FromEnd(position) => {
            if position > len {
                None
            } else {
                Some(len - position)
            }
        }
    }
}

pub trait DevFsDriver: Send + Sync + Debug + AsAny {
    fn driver_id(&self) -> u64;
    fn handles_device(&self, dev_fs: &mut DevFs, pci_device: &PciDevice) -> bool;
    fn refresh_device_hooks(
        &mut self,
        dev_fs: &mut DevFs,
        pci_device: &PciDevice,
        device_id: usize,
    ) -> Result<(), VfsError>;

    fn fopen(
        &mut self,
        dev_fs: &mut DevFs,
        file: Arc<DevFsHook>,
        mode: u64,
    ) -> Result<u64, VfsError>;
    fn fclose(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError>;
    fn fread(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError>;
    fn fwrite(&mut self, dev_fs: &mut DevFs, handle: u64, buf: &[u8]) -> Result<u64, VfsError>;
    fn fflush(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError>;
    fn fsync(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError>;
    fn fstat(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<FileStat, VfsError>;
    fn fseek(
        &mut self,
        dev_fs: &mut DevFs,
        handle: u64,
        position: SeekPosition,
    ) -> Result<u64, VfsError>;
}

#[derive(Debug, Clone)]
pub struct DevFsHandleData<T: Sized + Clone + Debug> {
    data: T,
    layout: Layout,
}

#[derive(Debug)]
pub enum DevFsHookKind {
    Device,
    SubBlockDevice { begin_block: u64, end_block: u64 },
    SubCharDevice { begin: u64, end: u64 },
}

#[derive(Debug)]
pub struct DevFsHook {
    pub driver: Arcrwb<dyn DevFsDriver>,
    pub file: VfsFile,
    pub kind: DevFsHookKind,
    pub generation: u64,
}

#[derive(Debug)]
pub struct DevFs {
    devices: Vec<PciDevice>,
    hooks: BTreeMap<Vec<char>, Arc<DevFsHook>>,
    handles: BTreeMap<u64, Arc<DevFsHook>>,

    drivers: BTreeMap<u64, Arcrwb<dyn DevFsDriver>>,

    os_id: u64,
    parent_fs_os_id: u64,
    mnt: Option<VfsFile>,
    root_fs: Option<WeakArcrwb<Vfs>>,
}

impl DevFs {
    pub fn register_driver(&mut self, driver: Arcrwb<dyn DevFsDriver>) -> Result<(), VfsError> {
        let mut guard = driver.write();
        let driver_id = guard.driver_id();

        if self.drivers.contains_key(&driver_id) {
            return Err(VfsError::ActionNotAllowed);
        }

        self.drivers.insert(driver_id, driver.clone());
        for (id, device) in self.devices.clone().iter().enumerate() {
            if guard.handles_device(self, device) {
                guard.refresh_device_hooks(self, device, id)?;
            }
        }

        Ok(())
    }

    /// Adds a hook to the devfs, and returns the previous one if any
    pub fn replace_hook(
        &mut self,
        path: Vec<char>,
        driver: u64,
        file: VfsFile,
        kind: DevFsHookKind,
        generation: u64,
    ) -> Option<Arc<DevFsHook>> {
        let driver = self.drivers.get(&driver)?.clone();
        let hook = Arc::new(DevFsHook {
            driver,
            file,
            kind,
            generation,
        });
        self.hooks.insert(path, hook.clone())
    }

    pub fn remove_hook(&mut self, path: &[char]) -> Option<Arc<DevFsHook>> {
        self.hooks.remove(path)
    }

    pub fn alloc_file_handle<T: Sized + Clone + Debug>(
        &mut self,
        data: T,
        hook: Arc<DevFsHook>,
    ) -> u64 {
        let handle = unsafe {
            let layout = Layout::from_size_align_unchecked(
                size_of::<DevFsHandleData<T>>(),
                align_of::<DevFsHandleData<T>>(),
            );
            let handle = alloc(layout) as *mut DevFsHandleData<T>;
            handle.write(DevFsHandleData { data, layout });
            handle as u64
        };
        self.handles.insert(handle, hook);
        handle
    }

    /// # Safety
    /// Caller must ensure that the handle is valid and was allocated using `alloc_file_handle` with the same `T` type
    pub unsafe fn get_handle_data<T: Sized + Clone + Debug>(&self, handle: u64) -> Option<*mut T> {
        let handle_data = handle as *mut DevFsHandleData<T>;
        Some(&mut (*handle_data).data as *mut T)
    }

    pub fn dealloc_file_handle<T: Sized + Clone + Debug>(&mut self, handle: u64) {
        if self.handles.contains_key(&handle) {
            unsafe {
                dealloc(
                    handle as *mut u8,
                    (*(handle as *mut DevFsHandleData<T>)).layout,
                )
            };
            self.handles.remove(&handle);
        }
    }
}

impl FileSystem for DevFs {
    fn get_root(&self) -> VfsFile {
        VfsFile::new(
            VfsFileKind::Directory,
            alloc::vec!['/'],
            0,
            self.parent_fs_os_id,
            self.os_id,
        )
    }

    fn os_id(&self) -> u64 {
        self.os_id
    }

    fn create_child(
        &mut self,
        _directory: &VfsFile,
        _name: &[char],
        _kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn get_child(&self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() != &['/'] {
            return Err(VfsError::PathNotFound);
        }

        let hook: Arc<DevFsHook> = self.hooks.get(child).ok_or(VfsError::PathNotFound)?.clone();

        Ok(hook.file.clone())
    }

    fn list_children(&self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() != &['/'] {
            return Ok(Vec::new());
        }
        Ok(self
            .hooks
            .values()
            .map(|hook| hook.file.clone())
            .collect::<Vec<_>>())
    }

    fn fs_type(&self) -> String {
        "devices".to_string()
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

    fn get_mount_point(&self) -> Result<Option<VfsFile>, VfsError> {
        Ok(Some(
            self.mnt
                .as_ref()
                .ok_or(VfsError::FileSystemNotMounted)?
                .clone(),
        ))
    }

    fn host_block_device(&self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
    }

    fn sub_file_systems(&self) -> Result<Vec<Arcrwb<dyn FileSystem>>, VfsError> {
        Ok(alloc::vec![])
    }

    fn mount(
        &mut self,
        _directory: &VfsFile,
        _name: &[char],
        _fs: Box<dyn FileSystem>,
    ) -> Result<VfsFile, VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn unmount(&mut self, _mount_point: &VfsFile) -> Result<(), VfsError> {
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
        Ok(self.get_root())
    }

    fn on_pre_unmount(&mut self) -> Result<bool, VfsError> {
        Ok(true)
    }

    fn on_unmount(&mut self) -> Result<(), VfsError> {
        self.mnt = None;
        self.os_id = 0;
        self.parent_fs_os_id = 0;
        Ok(())
    }

    fn get_vfs(&self) -> Result<WeakArcrwb<Vfs>, VfsError> {
        Ok(self
            .root_fs
            .as_ref()
            .ok_or(VfsError::FileSystemNotMounted)?
            .clone())
    }

    fn sync(&mut self) -> Result<(), VfsError> {
        Ok(())
    }

    fn fopen(&mut self, file: &VfsFile, mode: u64) -> Result<u64, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() == &['/'] {
            return Err(VfsError::ActionNotAllowed);
        }

        let hook = self
            .hooks
            .get(file.name())
            .cloned()
            .ok_or(VfsError::PathNotFound)?;

        let driver = hook.driver.clone();
        let handle = {
            let mut wguard = driver.write();
            // This automatically inserts the handle by calling `alloc_file_handle`
            (*wguard).fopen(self, hook, mode)?
        };

        Ok(handle)
    }

    fn fclose(&mut self, handle: u64) -> Result<(), VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                // This automatically removes the handle by calling `dealloc_file_handle`
                (*wguard).fclose(self, handle)
            }
        }
    }

    fn fseek(&mut self, handle: u64, position: SeekPosition) -> Result<u64, VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fseek(self, handle, position)
            }
        }
    }

    fn fwrite(&mut self, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fwrite(self, handle, buf)
            }
        }
    }

    fn fread(&mut self, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fread(self, handle, buf)
            }
        }
    }

    fn fflush(&mut self, handle: u64) -> Result<(), VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fflush(self, handle)
            }
        }
    }

    fn fsync(&mut self, handle: u64) -> Result<(), VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fsync(self, handle)?;
                // TODO: update device files
                Ok(())
            }
        }
    }

    fn fstat(&mut self, handle: u64) -> Result<FileStat, VfsError> {
        match self.handles.entry(handle) {
            Entry::Vacant(_) => Err(VfsError::ActionNotAllowed),
            Entry::Occupied(o) => {
                let driver = o.get().driver.clone();
                let mut wguard = driver.write();
                (*wguard).fstat(self, handle)
            }
        }
    }
}

pub fn init_devfs(vfs: &mut Vfs) {
    let fs = DevFs {
        devices: pci::get_devices(),
        hooks: BTreeMap::new(),
        handles: BTreeMap::new(),
        drivers: BTreeMap::new(),
        mnt: None,
        os_id: 0,
        parent_fs_os_id: 0,
        root_fs: None,
    };

    vfs.mount(&vfs.get_root(), &['d', 'e', 'v'], Box::new(fs))
        .unwrap();

    let fs: Arc<RwLock<Box<dyn FileSystem>>> = vfs
        .get_file(&['d', 'e', 'v'])
        .unwrap()
        .get_mounted_fs()
        .unwrap();

    let mut wguard = fs.write();
    let devfs = &mut **wguard;
    let devfs = devfs.as_any_mut().downcast_mut::<DevFs>().unwrap();
    init_disk_drivers(devfs);
    init_vga(devfs);
}
