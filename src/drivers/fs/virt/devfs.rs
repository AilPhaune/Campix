use core::fmt::Debug;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::RwLock;

use crate::drivers::{
    disk::init_disk_drivers,
    pci::{self, PciDevice},
    vfs::{
        Arcrwb, AsAny, BlockDevice, FileHandleAllocator, FileStat, FileSystem, PathTraverse,
        SeekPosition, Vfs, VfsError, VfsFile, VfsFileKind, VfsSpecificFileData, WeakArcrwb,
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
    fn ftruncate(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<u64, VfsError>;
    fn fflush(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError>;
    fn fsync(&mut self, dev_fs: &mut DevFs, handle: u64) -> Result<(), VfsError>;
    fn fstat(&mut self, dev_fs: &DevFs, handle: u64) -> Result<FileStat, VfsError>;
    fn fseek(
        &mut self,
        dev_fs: &mut DevFs,
        handle: u64,
        position: SeekPosition,
    ) -> Result<u64, VfsError>;
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
    pub device_id: u64,
}

#[derive(Debug)]
pub struct DevFs {
    devices: Vec<PciDevice>,
    hooks: BTreeMap<Vec<char>, Arc<DevFsHook>>,
    handles: FileHandleAllocator,

    drivers: BTreeMap<u64, Arcrwb<dyn DevFsDriver>>,

    os_id: u64,
    parent_fs_os_id: u64,
    mnt: Option<VfsFile>,
    root_fs: Option<WeakArcrwb<Vfs>>,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct DevFsHandleData<T: Sized + Clone + Debug> {
    hook: Arc<DevFsHook>,
    data: T,
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
        device_id: u64,
    ) -> Option<Arc<DevFsHook>> {
        let driver = self.drivers.get(&driver)?.clone();
        let hook = Arc::new(DevFsHook {
            driver,
            file,
            kind,
            generation,
            device_id,
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
        self.handles
            .alloc_file_handle::<DevFsHandleData<T>>(DevFsHandleData { data, hook })
    }

    /// # Safety
    /// Caller must ensure that the handle is valid and was allocated using `alloc_file_handle` with the same `T` type
    pub unsafe fn get_handle_data<T: Sized + Clone + Debug>(&self, handle: u64) -> Option<*mut T> {
        let handle_data = self.handles.get_handle_data::<DevFsHandleData<T>>(handle)?;
        Some(&mut (*handle_data).data as *mut T)
    }

    pub fn dealloc_file_handle<T: Sized + Clone + Debug>(&mut self, handle: u64) {
        self.handles
            .dealloc_file_handle::<DevFsHandleData<T>>(handle);
    }
}

macro_rules! get_hook {
    ($self: ident, $handle: ident) => {{
        unsafe {
            &*$self
                .handles
                .get_handle_data::<DevFsHandleData<()>>($handle)
                .ok_or(VfsError::BadHandle)?
        }
        .hook
        .clone()
    }};
}

macro_rules! get_driver {
    ($self: ident, $handle: ident) => {{
        get_hook!($self, $handle).driver.clone()
    }};
}

impl FileSystem for DevFs {
    fn get_root(&mut self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::Directory,
            alloc::vec!['/'],
            0,
            self.parent_fs_os_id,
            self.os_id,
            Arc::new(VfsSpecificFileData),
        ))
    }

    fn os_id(&mut self) -> u64 {
        self.os_id
    }

    fn fs_flush(&mut self) -> Result<(), VfsError> {
        Ok(())
    }

    fn create_child(
        &mut self,
        _directory: &VfsFile,
        _name: &[char],
        _kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError> {
        Err(VfsError::ReadOnly)
    }

    fn delete_file(&mut self, _file: &VfsFile) -> Result<(), VfsError> {
        Err(VfsError::ReadOnly)
    }

    fn get_child(&mut self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() != ['/'] {
            return Err(VfsError::PathNotFound);
        }

        let hook: Arc<DevFsHook> = self.hooks.get(child).ok_or(VfsError::PathNotFound)?.clone();

        Ok(hook.file.clone())
    }

    fn list_children(&mut self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() != ['/'] {
            return Ok(Vec::new());
        }
        Ok(self
            .hooks
            .values()
            .map(|hook| hook.file.clone())
            .collect::<Vec<_>>())
    }

    fn fs_type(&mut self) -> String {
        "devices".to_string()
    }

    fn get_file(&mut self, path: &[char]) -> Result<VfsFile, VfsError> {
        let mut traverse = PathTraverse::new_owned(path, self)?;
        loop {
            let result = traverse.find_next()?;
            if traverse.is_done() {
                break Ok(result);
            }
        }
    }

    fn get_stats(&mut self, file: &VfsFile) -> Result<FileStat, VfsError> {
        let handle = self.fopen(file, 0)?;
        let stats = self.fstat(handle);
        self.fclose(handle)?;
        stats
    }

    fn get_mount_point(&mut self) -> Result<Option<VfsFile>, VfsError> {
        Ok(Some(
            self.mnt
                .as_ref()
                .ok_or(VfsError::FileSystemNotMounted)?
                .clone(),
        ))
    }

    fn host_block_device(&mut self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
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
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() == ['/'] {
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
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        // This automatically removes the handle by calling `dealloc_file_handle`
        (*wguard).fclose(self, handle)
    }

    fn fseek(&mut self, handle: u64, position: SeekPosition) -> Result<u64, VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).fseek(self, handle, position)
    }

    fn fwrite(&mut self, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).fwrite(self, handle, buf)
    }

    fn fread(&mut self, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).fread(self, handle, buf)
    }

    fn ftruncate(&mut self, handle: u64) -> Result<u64, VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).ftruncate(self, handle)
    }

    fn fflush(&mut self, handle: u64) -> Result<(), VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).fflush(self, handle)
    }

    fn fsync(&mut self, handle: u64) -> Result<(), VfsError> {
        let hook = get_hook!(self, handle);
        let driver = hook.driver.clone();

        let device_id = hook.device_id as usize;
        let mut wguard = driver.write();
        (*wguard).fsync(self, handle)?;
        let device = *self
            .devices
            .get(device_id)
            .ok_or(VfsError::ActionNotAllowed)?;
        (*wguard).refresh_device_hooks(self, &device, device_id)?;

        Ok(())
    }

    fn fstat(&self, handle: u64) -> Result<FileStat, VfsError> {
        let driver = get_driver!(self, handle);

        let mut wguard = driver.write();
        (*wguard).fstat(self, handle)
    }
}

pub fn init_devfs(vfs: &mut Vfs) {
    let fs = DevFs {
        devices: pci::get_devices(),
        hooks: BTreeMap::new(),
        drivers: BTreeMap::new(),
        handles: FileHandleAllocator::default(),
        mnt: None,
        os_id: 0,
        parent_fs_os_id: 0,
        root_fs: None,
    };

    let dev = "dev".chars().collect::<Vec<char>>();

    vfs.mount(&dev, Box::new(fs)).unwrap();

    let fs: Arc<RwLock<Box<dyn FileSystem>>> =
        vfs.get_file(&dev).unwrap().get_mounted_fs().unwrap();

    let mut wguard = fs.write();
    let devfs = &mut **wguard;
    let devfs = devfs.as_any_mut().downcast_mut::<DevFs>().unwrap();
    init_disk_drivers(devfs);
    init_vga(devfs);
}
