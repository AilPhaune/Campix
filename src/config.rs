use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::{
    data::{alloc_boxed_slice, file::File, permissions::Permissions},
    drivers::vfs::OPEN_MODE_READ,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelBaseConfig {
    pub kernel_log_file: String,
    pub sysinit_stdout: String,
    pub sysinit_stderr: String,
}

pub const MAX_BASE_CONFIG_SIZE: u64 = 4096;

static mut KERNEL_CONFIG: Option<KernelBaseConfig> = None;

pub fn init_kernel_config() {
    let Some(stats) = File::get_stats("/system/etc/base").unwrap() else {
        panic!("Kernel base config at /system/etc/base not found !");
    };
    if stats.size > MAX_BASE_CONFIG_SIZE {
        panic!("Kernel base config at /system/etc/base too big !");
    }

    let base_file =
        File::open("/system/etc/base", OPEN_MODE_READ, Permissions::from_u64(0)).unwrap();

    let mut buffer = alloc_boxed_slice(stats.size as usize);

    let read = base_file.read(&mut buffer).unwrap();

    if read != stats.size {
        panic!(
            "Failed to read kernel base config at /system/etc/base, read {} bytes instead of {}",
            read, stats.size
        );
    }

    let config = match serde_json::from_slice(&buffer) {
        Ok(config) => config,
        Err(err) => {
            panic!(
                "Failed to parse kernel base config at /system/etc/base: {:#?}",
                err
            );
        }
    };

    unsafe {
        KERNEL_CONFIG = Some(config);
    }
}

#[allow(static_mut_refs)]
pub fn get_kernel_config() -> &'static KernelBaseConfig {
    unsafe { KERNEL_CONFIG.as_ref().unwrap() }
}
