use alloc::boxed::Box;

use crate::drivers::{
    fs::virt::{devfs::DevFs, files::dev_null::DevNullProvider},
    vfs::{arcrwb_new_from_box, FileSystem},
};

pub mod dev_null;

pub fn init_vfiles(devfs: &mut DevFs) {
    let os_id = devfs.os_id();

    devfs.insert_vfile(
        arcrwb_new_from_box(Box::new(DevNullProvider::new(os_id))),
        &['n', 'u', 'l', 'l'],
    );
}
