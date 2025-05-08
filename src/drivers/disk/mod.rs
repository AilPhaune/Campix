use alloc::boxed::Box;
use pata::{is_pata_device, PataDevfsDriver};

use super::{fs::virt::devfs::DevFs, pci, vfs::arcrwb_new_from_box};

pub mod pata;

pub fn init_disk_drivers(vfs: &mut DevFs) {
    if let Some(pci_device) = pci::device_iterator().find(|pci_device| is_pata_device(pci_device)) {
        vfs.register_driver(arcrwb_new_from_box(Box::new(PataDevfsDriver::new(
            *pci_device,
        ))))
        .unwrap();
    }
}
