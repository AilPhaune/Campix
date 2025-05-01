use alloc::boxed::Box;
use pata::PataDevfsDriver;

use super::{fs::virt::devfs::DevFS, vfs::arcrwb_new_from_box};

pub mod pata;

pub fn init_disk_drivers(vfs: &mut DevFS) {
    vfs.register_driver(arcrwb_new_from_box(Box::new(PataDevfsDriver::default())));
}
