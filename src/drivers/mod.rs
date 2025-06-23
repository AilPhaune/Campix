use crate::drivers::{disk::init_disk_drivers, fs::virt::devfs::DevFs, vga::init_vga};

pub mod disk;
pub mod fs;
pub mod keyboard;
pub mod pci;
pub mod ports;
pub mod time;
pub mod vfs;
pub mod vga;

pub fn init_vfiles(devfs: &mut DevFs) {
    init_vga(devfs);
    init_disk_drivers(devfs);

    ports::init_vfiles(devfs);
}
