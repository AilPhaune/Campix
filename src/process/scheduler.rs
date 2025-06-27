use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::{mutex::Mutex, RwLock};

use crate::{
    data::file::File,
    drivers::{fs::virt::pipefs::Pipe, vfs::VfsError},
    interrupts::handlers::syscall::linux::SIGKILL,
    paging::{get_kernel_page_table, PageTable, PAGE_ACCESSED, PAGE_PRESENT, PAGE_RW},
    percpu::{core_id, get_per_cpu, InterruptSource},
    process::{io::context::ProcessIOContext, ui::context::UiContext},
};

use super::{
    memory::{ProcessHeap, ThreadStack, PROC_KERNEL_STACK_TOP},
    proc::{Process, ProcessAccess, ProcessAllocatedCode, TaskState, Thread, ThreadState},
};

#[derive(Debug, Clone)]
pub struct ProcThreadInfo {
    pub thread: Arc<Thread>,
    pub pid: u32,
    pub tid: u32,
}

#[derive(Debug)]
pub struct SchedulerProcessCreateState {
    next_pid: u32,
}

#[derive(Debug)]
pub struct Scheduler {
    processes: RwLock<BTreeMap<u32, Arc<Process>>>,
    threads: RwLock<BTreeMap<u32, ProcThreadInfo>>,
    proc_create_state: Mutex<SchedulerProcessCreateState>,

    task_queue: Mutex<VecDeque<ProcThreadInfo>>,

    thread_settings: Mutex<SchedulerThreadSettings>,

    focused_thread: Mutex<Option<ProcThreadInfo>>,
}

#[derive(Debug, Clone)]
pub struct SchedulerThreadSettings {
    pub default_user_stack_pages: u64,
    pub default_kernel_stack_pages: u64,
    pub max_user_stack_pages: u64,
    pub max_kernel_stack_pages: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub const fn new() -> Scheduler {
        Scheduler {
            processes: RwLock::new(BTreeMap::new()),
            threads: RwLock::new(BTreeMap::new()),
            proc_create_state: Mutex::new(SchedulerProcessCreateState { next_pid: 1 }),

            task_queue: Mutex::new(VecDeque::new()),

            thread_settings: Mutex::new(SchedulerThreadSettings {
                default_user_stack_pages: 1,
                default_kernel_stack_pages: 1,
                max_user_stack_pages: 256,
                max_kernel_stack_pages: 32,
            }),

            focused_thread: Mutex::new(None),
        }
    }

    pub fn get_process(&self, pid: u32) -> Option<Arc<Process>> {
        self.processes.read().get(&pid).cloned()
    }

    pub fn get_thread(&self, tid: u32) -> Option<ProcThreadInfo> {
        self.threads.read().get(&tid).cloned()
    }

    pub fn get_next_pid(&self) -> u32 {
        let mut guard = self.proc_create_state.lock();
        let rguard = self.threads.read();
        loop {
            let pid = guard.next_pid;
            guard.next_pid += 1;
            if !rguard.contains_key(&pid) {
                return pid;
            }
        }
    }

    pub fn create_process(
        &self,
        options: CreateProcessOptions,
        stdin: File,
        stdout_override: Option<(File, File)>,
        stderr_override: Option<(File, File)>,
    ) -> Result<(u32, File, File), VfsError> {
        let pid = self.get_next_pid();

        let pml4 = options.page_table.get_pml4();

        let stdout = match stdout_override {
            Some(pipe) => pipe,
            None => {
                let pipe = Pipe::create()?;
                (pipe.1, pipe.2)
            }
        };
        let stderr = match stderr_override {
            Some(pipe) => pipe,
            None => {
                let pipe = Pipe::create()?;
                (pipe.1, pipe.2)
            }
        };

        let process = Arc::new(Process {
            name: options.name.clone(),
            cmdline: options.cmdline,
            cwd: Mutex::new(options.cwd),
            pid,
            page_table: Mutex::new(options.page_table),
            pml4,
            heap: Mutex::new(ProcessHeap::new()),
            uid: options.uid,
            gid: options.gid,
            effective_process_access: Mutex::new(ProcessAccess {
                euid: options.uid,
                egid: options.gid,
                supplementary_gids: options.supplementary_gids,
            }),
            allocated_code: Mutex::new(options.allocated_code),
            syscalls: Mutex::new(options.syscalls),
            threads: Mutex::new(Vec::new()),
            zombie_threads: Mutex::new(Vec::new()),
            state: Mutex::new(TaskState::Init),
            io_context: Mutex::new(ProcessIOContext::new_with_stdio(stdin, stdout.1, stderr.1)),
        });

        let mut pt = process.page_table.lock();

        let thread = Arc::new(Thread {
            pid,
            tid: pid,
            name: options.name,
            process: process.clone(),
            kernel_stack: Mutex::new(ThreadStack::new_with_pages(
                PROC_KERNEL_STACK_TOP,
                1,
                &mut pt,
                PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED,
            )),
            stack: Mutex::new(options.main_thread_stack),
            state: Mutex::new(options.main_thread_state),
            running_cpu: Mutex::new(None),
            task_state: Mutex::new(TaskState::Init),
            ui_context: Mutex::new(UiContext::pid_tid(pid, pid)),
        });

        drop(pt);

        let mut lock = process.threads.lock();
        lock.push(thread.clone());
        drop(lock);

        let proct = ProcThreadInfo {
            thread: thread.clone(),
            pid,
            tid: pid,
        };

        self.processes.write().insert(pid, process.clone());
        self.threads.write().insert(pid, proct.clone());
        self.task_queue.lock().push_back(proct);

        Ok((pid, stdout.0, stderr.0))
    }

    pub fn get_thread_settings(&self) -> SchedulerThreadSettings {
        let guard = self.thread_settings.lock();
        let value = (*guard).clone();
        drop(guard);
        value
    }

    pub fn handle_exit(&self, tid: u32, exit_code: u64) {
        self.handle_exit_internal::<true>(tid, exit_code)
    }

    fn handle_exit_internal<const H: bool>(&self, tid: u32, exit_code: u64) {
        let mut kpt = get_kernel_page_table().lock();
        unsafe {
            kpt.load();
        }
        drop(kpt);

        let lock = self.threads.write();
        if let Some(t) = lock.get(&tid) {
            let thread: Arc<Thread> = t.thread.clone();
            drop(lock);

            let mut ptlock = thread.process.page_table.lock();
            let pt: &mut PageTable = &mut ptlock;

            let mut lock = thread.running_cpu.lock();
            *lock = None;
            drop(lock);

            let mut lock = thread.stack.lock();
            lock.free(pt);
            drop(lock);

            let mut lock = thread.process.threads.lock();
            lock.retain(|t| t.tid != tid);

            let last = lock.is_empty();
            drop(lock);

            let mut lock = thread.process.zombie_threads.lock();
            lock.push(thread.clone());
            drop(lock);

            let mut lock = thread.task_state.lock();
            *lock = TaskState::Zombie { exit_code };
            drop(lock);

            if H && last {
                drop(ptlock);

                self.handle_process_exit(thread.process.pid, exit_code);
            }
        }
    }

    pub fn handle_process_exit(&self, pid: u32, exit_code: u64) {
        let mut kpt = get_kernel_page_table().lock();
        unsafe {
            kpt.load();
        }
        drop(kpt);

        let mut lock = self.processes.write();
        if let Some(p) = lock.remove(&pid) {
            let process: Arc<Process> = p;
            drop(lock);

            let mut ptlock = process.page_table.lock();
            let pt: &mut PageTable = &mut ptlock;

            let mut lock = process.allocated_code.lock();
            lock.free(pt);
            drop(lock);

            let mut lock = process.heap.lock();
            lock.free(pt);
            drop(lock);

            let lock = process.threads.lock();
            let proc_tids = lock.iter().map(|t| t.tid).collect::<Vec<u32>>();
            drop(lock);

            for t in proc_tids.into_iter() {
                drop(ptlock);
                self.handle_exit_internal::<false>(t, exit_code);
                ptlock = process.page_table.lock();
            }

            drop(ptlock);

            let mut lock = process.state.lock();
            *lock = TaskState::Zombie { exit_code };
            drop(lock);
        }
    }

    pub fn get_zombie_thread(&self, tid: u32) -> Option<ProcThreadInfo> {
        self.threads.read().get(&tid).cloned()
    }

    pub fn remove_zombie(&self, tid: u32) {
        self.threads.write().remove(&tid);
    }

    pub fn kill_process(&self, pid: u32) {
        let lock = self.processes.read();
        let proc_syscall_abi = match lock.get(&pid) {
            Some(p) => {
                let syslock = p.syscalls.lock();
                let abi = *syslock;
                drop(syslock);
                abi
            }
            None => {
                drop(lock);
                return;
            }
        };
        drop(lock);

        match proc_syscall_abi {
            ProcessSyscallABI::Linux => {
                self.handle_process_exit(pid, 128 + SIGKILL);
            }
        }
    }

    pub fn schedule(&self) -> ! {
        unsafe {
            core::arch::asm!("cli");
        }
        'outer: loop {
            let mut guard = self.task_queue.lock();

            let per_cpu = get_per_cpu();
            if let (Some(InterruptSource::User | InterruptSource::Syscall), Some(thread)) =
                (per_cpu.interrupt_sources.last(), &per_cpu.running_thread)
            {
                let mut ok = false;
                let slock = thread.thread.task_state.lock();
                if !matches!(*slock, TaskState::Zombie { .. }) {
                    let plock = thread.thread.process.state.lock();
                    if !matches!(*plock, TaskState::Zombie { .. }) {
                        ok = true;
                    }
                    drop(plock);
                }
                drop(slock);
                if ok {
                    guard.push_back(thread.clone());
                }
            }
            let thread: Option<ProcThreadInfo> = guard.pop_front();
            drop(guard);

            if let (Some(InterruptSource::Syscall), Some(running)) =
                (per_cpu.interrupt_sources.last(), &per_cpu.running_thread)
            {
                let mut state = running.thread.state.lock();

                state.gpregs.rax = per_cpu.syscall_data.rax;
                state.gpregs.rbx = per_cpu.syscall_data.rbx;
                state.gpregs.rdx = per_cpu.syscall_data.rdx;
                state.gpregs.rsi = per_cpu.syscall_data.rsi;
                state.gpregs.rdi = per_cpu.syscall_data.rdi;
                state.gpregs.r8 = per_cpu.syscall_data.r8;
                state.gpregs.r9 = per_cpu.syscall_data.r9;
                state.gpregs.r10 = per_cpu.syscall_data.r10;
                state.gpregs.r12 = per_cpu.syscall_data.r12;
                state.gpregs.r13 = per_cpu.syscall_data.r13;
                state.gpregs.r14 = per_cpu.syscall_data.r14;
                state.gpregs.r15 = per_cpu.syscall_data.r15;

                state.rip = per_cpu.syscall_data.rcx; // Syscall return address
                state.rsp = per_cpu.syscall_data.rsp; // Syscall process stack
                state.rbp = per_cpu.syscall_data.rbp; // Syscall process stack base
                state.rflags = per_cpu.syscall_data.r11; // Syscall rflags

                drop(state);
            }

            if let Some(thread) = thread {
                let plock = self.processes.read();
                if let Some(process) = plock.get(&thread.pid) {
                    let mut slock = process.state.lock();
                    if matches!(*slock, TaskState::Zombie { .. }) {
                        continue 'outer;
                    }
                    *slock = TaskState::Running;
                    drop(slock);
                }
                drop(plock);

                let mut tlock = thread.thread.task_state.lock();
                *tlock = TaskState::Running;
                drop(tlock);

                let mut lock = thread.thread.running_cpu.lock();
                *lock = Some(core_id());
                // Guard is not dropped here, it will be dropped when an interrupt interrupts this thread
                core::mem::forget(lock);

                per_cpu.running_thread = Some(thread);
                if let Some(thread) = &per_cpu.running_thread {
                    thread.thread.jmp_to_userland();
                } else {
                    unreachable!("Running proc is not set");
                }
            }

            // If there are no threads to run, sleep
            // This loop will be interrupted by any next interrupt (probably a timer interrupt which will reschedule and never return to here)
            unsafe {
                core::arch::asm!("sti");
            }
            loop {
                core::hint::spin_loop();
            }
        }
    }

    pub fn get_focused_thread(&self) -> Option<ProcThreadInfo> {
        let lock = self.focused_thread.lock();
        let value = (*lock).clone();
        drop(lock);
        value
    }
}

#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum ProcessSyscallABI {
    Linux = 0,
}

#[derive(Debug)]
pub struct CreateProcessOptions {
    pub name: String,
    pub cmdline: Vec<String>,
    pub cwd: String,

    pub uid: u32,
    pub gid: u32,
    pub supplementary_gids: Vec<u32>,

    pub page_table: PageTable,

    pub main_thread_state: ThreadState,

    pub allocated_code: ProcessAllocatedCode,
    pub syscalls: ProcessSyscallABI,

    pub main_thread_stack: ThreadStack,
}

pub static SCHEDULER: Scheduler = Scheduler::new();
