use processes::linux_sys_exit;

use crate::{
    interrupts::{
        handlers::syscall::linux::{io::linux_sys_write, processes::linux_sys_sched_yield},
        idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    },
    process::scheduler::ProcThreadInfo,
};

pub mod io;
pub mod processes;

pub const EINVAL: u64 = 22;
pub const ENOSYS: u64 = 38;

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
}

#[inline(always)]
pub fn linux_return_err_from_syscall(
    ifr: &mut InterruptFrameRegisters,
    thread: &ProcThreadInfo,
    err: u64,
) {
    linux_return_from_syscall(ifr, thread, -(err as i64) as u64);
}

pub fn linux_syscall(
    ifr: &mut InterruptFrameRegisters,
    _ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
    thread: &mut ProcThreadInfo,
) -> bool {
    let intno = ifr.rax;
    let arg0 = ifr.rdi;
    let arg1 = ifr.rsi;
    let arg2 = ifr.rdx;
    let _arg3 = ifr.r10;
    let _arg4 = ifr.r8;
    let _arg5 = ifr.r9;
    match intno {
        1 => linux_sys_write(ifr, thread, arg0, arg1, arg2),
        24 => linux_sys_sched_yield(ifr, thread),
        60 => linux_sys_exit(thread.tid, arg0),
        _ => {
            linux_return_from_syscall(ifr, thread, ENOSYS);
        }
    }
    true
}
