use crate::{
    interrupts::handlers::syscall::{linux::EINVAL, utils::structure::UserProcessStructure},
    linux_return_err_from_syscall,
    process::scheduler::ProcThreadInfo,
};

pub struct LinuxUtsname {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
}

macro_rules! populate_cstr {
    ($value: expr, $slice: expr) => {{
        let len = $value.len();
        if len + 1 > $slice.len() {
            false
        } else {
            $slice[0..len].copy_from_slice($value);
            $slice[len..].fill(0);
            true
        }
    }};
}

pub fn linux_sys_uname(thread: &ProcThreadInfo, buf: u64) -> u64 {
    let mut ptlock = thread.thread.process.page_table.lock();
    let Some(mut user_struct) = UserProcessStructure::new(buf as *mut LinuxUtsname) else {
        linux_return_err_from_syscall!(EINVAL)
    };
    match user_struct.verify_fully_mapped_mut(&mut ptlock) {
        Some(utsname) => {
            if !populate_cstr!(b"Campix", utsname.sysname)
                || !populate_cstr!(b"Campix", utsname.nodename)
                || !populate_cstr!(b"0.1", utsname.release)
                || !populate_cstr!(b"0.1", utsname.version)
                || !populate_cstr!(b"x86_64", utsname.machine)
            {
                linux_return_err_from_syscall!(EINVAL)
            } else {
                0
            }
        }
        None => linux_return_err_from_syscall!(EINVAL),
    }
}
