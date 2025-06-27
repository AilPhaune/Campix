use crate::{
    data::regs::fs_gs_base::{FsBase, KernelGsBase},
    interrupts::handlers::syscall::{
        linux::{EINVAL, EPERM},
        utils::structure::UserProcessStructure,
    },
    linux_return_err_from_syscall,
    paging::PageTable,
    process::scheduler::{ProcThreadInfo, SCHEDULER},
};

pub fn linux_sys_exit(tid: u32, code: u64) -> ! {
    SCHEDULER.handle_exit(tid, (code & 0xFF) << 8);
    SCHEDULER.schedule()
}

pub fn linux_sys_get_pid(thread: &ProcThreadInfo) -> u64 {
    thread.pid as u64
}

pub fn linux_sys_get_tid(thread: &ProcThreadInfo) -> u64 {
    thread.tid as u64
}

pub fn linux_sys_sched_yield(thread: &ProcThreadInfo) -> ! {
    let mut state = thread.thread.state.lock();
    state.gpregs.rax = 0;
    drop(state);

    SCHEDULER.schedule();
}

pub const ARCH_SET_GS: u64 = 0x1001;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const ARCH_GET_GS: u64 = 0x1004;

pub fn linux_sys_arch_prctl(thread: &ProcThreadInfo, code: u64, value: u64) -> u64 {
    match code {
        // TODO: ARCH_SET_CPUID
        // TODO: ARCH_GET_CPUID
        ARCH_SET_FS => {
            thread.thread.state.lock().fs_base = value;
            unsafe {
                FsBase::set(value);
            }
            0
        }
        ARCH_GET_FS => match UserProcessStructure::<u64>::new(value as *mut u64) {
            Some(mut user_u64) => {
                match user_u64.verify_fully_mapped_mut(&mut PageTable::temporary_this()) {
                    Some(fs_base_ptr) => {
                        *fs_base_ptr = thread.thread.state.lock().fs_base;
                        0
                    }
                    None => linux_return_err_from_syscall!(EPERM),
                }
            }
            None => {
                linux_return_err_from_syscall!(EPERM)
            }
        },
        ARCH_SET_GS => {
            thread.thread.state.lock().gs_base = value;
            unsafe {
                // Currently used gs base is the kernel one, when spawgs is run when switching back to user mode, user process will get the correct gs base
                KernelGsBase::set(value);
            }
            0
        }
        ARCH_GET_GS => match UserProcessStructure::<u64>::new(value as *mut u64) {
            Some(mut user_u64) => {
                match user_u64.verify_fully_mapped_mut(&mut PageTable::temporary_this()) {
                    Some(gs_base_ptr) => {
                        *gs_base_ptr = thread.thread.state.lock().gs_base;
                        0
                    }
                    None => linux_return_err_from_syscall!(EPERM),
                }
            }
            None => {
                linux_return_err_from_syscall!(EPERM)
            }
        },
        _ => {
            linux_return_err_from_syscall!(EINVAL)
        }
    }
}
