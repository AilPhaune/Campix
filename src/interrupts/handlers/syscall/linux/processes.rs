use crate::{
    interrupts::{
        handlers::syscall::linux::linux_return_from_syscall, idt::InterruptFrameRegisters,
    },
    process::scheduler::{ProcThreadInfo, SCHEDULER},
};

pub fn linux_sys_exit(tid: u32, code: u64) -> ! {
    SCHEDULER.handle_exit(tid, (code & 0xFF) << 8);
    SCHEDULER.schedule()
}

pub fn linux_sys_sched_yield(ifr: &mut InterruptFrameRegisters, thread: &ProcThreadInfo) -> ! {
    linux_return_from_syscall(ifr, thread, 0);
    SCHEDULER.schedule();
}
