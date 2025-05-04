use core::fmt::Debug;

use alloc::{
    boxed::Box,
    collections::{btree_map, BTreeMap},
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::RwLock;

use crate::drivers::{
    disk::init_disk_drivers,
    pci::{self, PciDevice},
    vfs::{
        Arcrwb, AsAny, BlockDevice, FileSystem, PathTraverse, Vfs, VfsError, VfsFile, VfsFileKind,
        WeakArcrwb,
    },
    vga::init_vga,
};

pub trait DevFsDriver: Send + Sync + Debug + AsAny {
    fn driver_id(&self) -> u64;
    fn handles_device(&self, dev_fs: &mut DevFS, pci_device: &PciDevice) -> bool;
    fn get_device_files(
        &self,
        dev_fs: &mut DevFS,
        pci_device: &PciDevice,
    ) -> Result<Vec<VfsFile>, VfsError>;
}

#[derive(Debug)]
pub struct DevFS {
    devices: Vec<PciDevice>,
    drivers: BTreeMap<u64, Arcrwb<dyn DevFsDriver>>,
    dev_to_driver: BTreeMap<usize, u64>,
    dev_to_file: BTreeMap<usize, Vec<VfsFile>>,
    name_to_dev: BTreeMap<Vec<char>, usize>,

    os_id: u64,
    parent_fs_os_id: u64,
    mnt: Option<VfsFile>,
    root_fs: Option<WeakArcrwb<Vfs>>,
}

impl DevFS {
    pub fn register_driver(&mut self, driver: Arcrwb<dyn DevFsDriver>) -> bool {
        let id = driver.read().driver_id();
        if let btree_map::Entry::Vacant(e) = self.drivers.entry(id) {
            e.insert(driver);
            self.init_driver(id)
        } else {
            false
        }
    }

    fn init_driver(&mut self, id: u64) -> bool {
        let driver = self.drivers.get(&id).unwrap().clone();
        for (i, device) in self.devices.clone().iter().enumerate() {
            if driver.read().handles_device(self, device)
                && !self.init_driver_for_device(i, device, id, driver.clone())
            {
                return false;
            }
        }
        true
    }

    fn init_driver_for_device(
        &mut self,
        index: usize,
        device: &PciDevice,
        driver_id: u64,
        driver: Arcrwb<dyn DevFsDriver>,
    ) -> bool {
        if let btree_map::Entry::Vacant(e) = self.dev_to_driver.entry(index) {
            e.insert(driver_id);
            let files: Vec<VfsFile> = driver.read().get_device_files(self, device).unwrap();
            for f in files.iter() {
                self.name_to_dev.insert(f.name().clone(), index);
            }
            self.dev_to_file.insert(index, files);

            true
        } else {
            false
        }
    }
}

impl FileSystem for DevFS {
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

        let device = self
            .name_to_dev
            .get(child)
            .cloned()
            .ok_or(VfsError::PathNotFound)?;

        let file = self
            .dev_to_file
            .get(&device)
            .unwrap()
            .iter()
            .find(|f| f.name() == child);

        file.cloned().ok_or(VfsError::PathNotFound)
    }

    fn list_children(&self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        if file.name() != &['/'] {
            return Ok(Vec::new());
        }
        Ok(self.dev_to_file.values().flatten().cloned().collect())
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
}

pub fn init_devfs(vfs: &mut Vfs) {
    let fs = DevFS {
        devices: pci::get_devices(),
        drivers: BTreeMap::new(),
        dev_to_driver: BTreeMap::new(),
        dev_to_file: BTreeMap::new(),
        name_to_dev: BTreeMap::new(),
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
    let devfs = devfs.as_any_mut().downcast_mut::<DevFS>().unwrap();
    init_disk_drivers(devfs);
    init_vga(devfs);
}
