use processes::linux_sys_exit;

use crate::{
    drivers::vfs::VfsError,
    interrupts::{
        handlers::syscall::linux::{
            io::{linux_sys_close, linux_sys_open, linux_sys_write},
            processes::linux_sys_sched_yield,
        },
        idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    },
    percpu::get_per_cpu,
    process::scheduler::ProcThreadInfo,
};

pub mod io;
pub mod processes;

pub const EPERM: u64 = 1;
pub const ENOENT: u64 = 2;
pub const EIO: u64 = 5;
pub const EBADF: u64 = 9;
pub const EEXIST: u64 = 17;
pub const ENOTDIR: u64 = 20;
pub const EISDIR: u64 = 21;
pub const EINVAL: u64 = 22;
pub const EMFILE: u64 = 24;
pub const ESPIPE: u64 = 29;
pub const ENOSYS: u64 = 38;
pub const ENOTEMPTY: u64 = 39;

pub const SIGKILL: u64 = 9;

#[inline(always)]
pub fn linux_return_from_syscall(
    ifr: &mut InterruptFrameRegisters,
    thread: &ProcThreadInfo,
    rax: u64,
) {
    let mut state = thread.thread.state.lock();
    state.gpregs.rax = rax;
    ifr.rax = rax;
    drop(state);
    get_per_cpu().syscall_data.rax = rax;
}

#[macro_export]
macro_rules! linux_return_err_from_syscall {
    ($err: expr) => {
        return (-($err as i64)) as u64
    };
}

#[inline(always)]
#[allow(clippy::too_many_arguments)] // stfu
fn linux_syscall0(
    intno: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
    _arg5: u64,
    thread: &ProcThreadInfo,
) -> u64 {
    match intno {
        1 => linux_sys_write(thread, arg0, arg1, arg2),
        2 => linux_sys_open(thread, arg0, arg1, arg2),
        3 => linux_sys_close(thread, arg0),
        24 => linux_sys_sched_yield(thread),
        60 => linux_sys_exit(thread.tid, arg0),
        _ => (-(ENOSYS as i64)) as u64,
    }
}

pub fn linux_syscall(
    ifr: &mut InterruptFrameRegisters,
    _ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
    thread: &ProcThreadInfo,
) -> bool {
    let res = linux_syscall0(
        ifr.rax, ifr.rdi, ifr.rsi, ifr.rdx, ifr.r10, ifr.r8, ifr.r9, thread,
    );
    linux_return_from_syscall(ifr, thread, res);
    (res as i64) >= 0
}

pub fn linux_syscall_fast(thread: &ProcThreadInfo) -> bool {
    let percpu = get_per_cpu();

    let res = linux_syscall0(
        percpu.syscall_data.rax,
        percpu.syscall_data.rdi,
        percpu.syscall_data.rsi,
        percpu.syscall_data.rdx,
        percpu.syscall_data.r10,
        percpu.syscall_data.r8,
        percpu.syscall_data.r9,
        thread,
    );

    percpu.syscall_data.rax = res;
    let mut state = thread.thread.state.lock();
    state.gpregs.rax = res;
    drop(state);

    (res as i64) >= 0
}

pub fn vfs_err_to_linux_errno(err: VfsError) -> u64 {
    match err {
        VfsError::PathNotFound | VfsError::EntryNotFound => ENOENT,
        VfsError::InvalidArgument | VfsError::BadBufferSize | VfsError::InvalidOpenMode => EINVAL,
        VfsError::InvalidSeekPosition => ESPIPE,
        VfsError::ActionNotAllowed => EPERM,
        VfsError::BadHandle => EBADF,
        VfsError::FileAlreadyExists => EEXIST,
        VfsError::DirectoryNotEmpty => ENOTEMPTY,
        VfsError::NotDirectory => ENOTDIR,
        VfsError::NotFile => EISDIR,
        _ => EIO,
    }
}
