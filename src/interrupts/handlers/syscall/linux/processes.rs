use crate::process::scheduler::{ProcThreadInfo, SCHEDULER};

pub fn linux_sys_exit(tid: u32, code: u64) -> ! {
    SCHEDULER.handle_exit(tid, (code & 0xFF) << 8);
    SCHEDULER.schedule()
}

pub fn linux_sys_sched_yield(thread: &ProcThreadInfo) -> ! {
    let mut state = thread.thread.state.lock();
    state.gpregs.rax = 0;
    drop(state);

    SCHEDULER.schedule();
}
