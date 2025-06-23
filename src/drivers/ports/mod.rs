use crate::drivers::fs::virt::devfs::DevFs;

pub mod e9;
pub mod parallel;

pub fn init_vfiles(devfs: &mut DevFs) {
    parallel::init_lpt_files(devfs);
    e9::init_e9_file(devfs);
}
