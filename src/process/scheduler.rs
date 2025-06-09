use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::{mutex::Mutex, RwLock};

use crate::{
    interrupts::handlers::syscall::linux::SIGKILL,
    paging::{get_kernel_page_table, PageTable, PAGE_ACCESSED, PAGE_PRESENT, PAGE_RW, PAGE_USER},
    percpu::{core_id, get_per_cpu},
};

use super::{
    memory::{ProcessHeap, ThreadStack, PROC_KERNEL_STACK_TOP, PROC_USER_STACK_TOP},
    proc::{Process, ProcessAccess, ProcessAllocatedCode, TaskState, Thread, ThreadState},
};

#[derive(Debug, Clone)]
pub struct ProcThreadInfo {
    pub thread: Thread,
    pub pid: u32,
    pub tid: u32,
}

#[derive(Debug)]
pub struct SchedulerProcessCreateState {
    next_pid: u32,
}

#[derive(Debug)]
pub struct Scheduler {
    processes: RwLock<BTreeMap<u32, Process>>,
    threads: RwLock<BTreeMap<u32, ProcThreadInfo>>,
    proc_create_state: Mutex<SchedulerProcessCreateState>,

    task_queue: Mutex<VecDeque<ProcThreadInfo>>,

    thread_settings: Mutex<SchedulerThreadSettings>,
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
        }
    }

    pub fn get_process(&self, pid: u32) -> Option<Process> {
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

    pub fn create_process(&self, options: CreateProcessOptions) -> u32 {
        let pid = self.get_next_pid();

        let name = Arc::new(options.name);

        let process = Process {
            name: name.clone(),
            cmdline: Arc::new(options.cmdline),
            cwd: Arc::new(Mutex::new(options.cwd)),
            pid,
            page_table: Arc::new(Mutex::new(options.page_table)),
            heap: Arc::new(Mutex::new(ProcessHeap::new())),
            uid: options.uid,
            gid: options.gid,
            effective_process_access: Arc::new(Mutex::new(ProcessAccess {
                euid: options.uid,
                egid: options.gid,
                supplementary_gids: options.supplementary_gids,
            })),
            allocated_code: Arc::new(Mutex::new(options.allocated_code)),
            syscalls: Arc::new(Mutex::new(options.syscalls)),
            threads: Arc::new(Mutex::new(Vec::new())),
            zombie_threads: Arc::new(Mutex::new(Vec::new())),
            state: Arc::new(Mutex::new(TaskState::Init)),
        };

        let mut pt = process.page_table.lock();

        let thread = Thread {
            pid,
            tid: pid,
            name,
            process: process.clone(),
            kernel_stack: Arc::new(Mutex::new(ThreadStack::new_with_pages(
                PROC_KERNEL_STACK_TOP,
                1,
                &mut pt,
                PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED,
            ))),
            stack: Arc::new(Mutex::new(ThreadStack::new_with_pages(
                PROC_USER_STACK_TOP,
                1,
                &mut pt,
                PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED | PAGE_USER,
            ))),
            state: Arc::new(Mutex::new(options.main_thread_state)),
            running_cpu: Arc::new(Mutex::new(None)),
            task_state: Arc::new(Mutex::new(TaskState::Init)),
        };

        drop(pt);

        let mut lock = process.threads.lock();
        lock.push(thread.clone());
        drop(lock);

        let proct = ProcThreadInfo {
            thread,
            pid,
            tid: pid,
        };

        self.processes.write().insert(pid, process.clone());
        self.threads.write().insert(pid, proct.clone());
        self.task_queue.lock().push_back(proct);

        pid
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
            let thread: Thread = t.thread.clone();
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
            let process: Process = p;
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

            let thread: Option<ProcThreadInfo> = guard.pop_front();

            let per_cpu = get_per_cpu();
            if let (Some(true), Some(pid), Some(tid)) = (
                per_cpu.interrupted_from_userland.last(),
                per_cpu.running_pid,
                per_cpu.running_tid,
            ) {
                if let Some(thread) = self.get_thread(tid) {
                    if thread.pid == pid {
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
                            guard.push_back(thread);
                        }
                    }
                }
            }
            drop(guard);

            per_cpu.running_pid = None;
            per_cpu.running_tid = None;

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

                thread.thread.jmp_to_userland();
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
}

#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum ProcessSyscallABI {
    Linux = 0,
}

#[derive(Debug)]
pub struct CreateProcessOptions {
    pub name: String,
    pub cmdline: String,
    pub cwd: String,

    pub uid: u32,
    pub gid: u32,
    pub supplementary_gids: Vec<u32>,

    pub page_table: PageTable,

    pub main_thread_state: ThreadState,

    pub allocated_code: ProcessAllocatedCode,
    pub syscalls: ProcessSyscallABI,
}

pub static SCHEDULER: Scheduler = Scheduler::new();
