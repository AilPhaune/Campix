use crate::{
    drivers::vfs::{get_vfs, FileSystem},
    e9::write_char,
    interrupts::handlers::syscall::{linux::EINVAL, utils::buffer::UserProcessBuffer},
    linux_return_err_from_syscall,
    process::{
        memory::{get_address_space, VirtualAddressSpace},
        scheduler::ProcThreadInfo,
    },
};

const MAX_TTY_WRITE_SIZE: u64 = 4096; // 4KiB
const MAX_SINGLE_WRITE: u64 = 64 * 1024 * 1024; // 64MiB

pub fn linux_sys_write(thread: &ProcThreadInfo, fd: u64, buf: u64, count: u64) -> u64 {
    if count > MAX_SINGLE_WRITE {
        linux_return_err_from_syscall!(EINVAL)
    }

    let space = get_address_space(buf);
    let Some(end_addr) = buf.checked_add(count) else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let end_space = get_address_space(end_addr);
    if !matches!(space, Some(VirtualAddressSpace::LowerHalf(..)))
        || !matches!(end_space, Some(VirtualAddressSpace::LowerHalf(..)))
    {
        linux_return_err_from_syscall!(EINVAL)
    }

    match fd {
        1 | 2 => {
            // stdout, stderr
            let mut ptlock = thread.thread.process.page_table.lock();
            let written = count.min(MAX_TTY_WRITE_SIZE);
            let user_buffer = UserProcessBuffer::new(buf as *mut u8, written as usize);
            let mapped = user_buffer.verify_fully_mapped(&mut ptlock);
            drop(ptlock);
            match mapped {
                Some(buf) => {
                    for c in buf.iter() {
                        write_char(*c);
                    }
                    written
                }
                None => {
                    linux_return_err_from_syscall!(EINVAL)
                }
            }
        }
        0 => linux_return_err_from_syscall!(EINVAL),
        _ => {
            let mut ptlock = thread.thread.process.page_table.lock();
            let user_buffer = UserProcessBuffer::new(buf as *mut u8, count as usize);
            match user_buffer.verify_fully_mapped(&mut ptlock) {
                Some(buf) => {
                    let vfs = get_vfs();
                    let mut lock = vfs.write();
                    match lock.fwrite(fd, buf) {
                        Ok(written) => written,
                        Err(_) => {
                            // TODO: return error
                            linux_return_err_from_syscall!(EINVAL)
                        }
                    }
                }
                None => {
                    linux_return_err_from_syscall!(EINVAL)
                }
            }
        }
    }
}
