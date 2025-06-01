use alloc::{
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
};
use spin::{mutex::Mutex, RwLock};

use crate::{
    paging::{PageTable, PAGE_ACCESSED, PAGE_PRESENT, PAGE_RW, PAGE_USER},
    percpu::{core_id, get_per_cpu},
};

use super::{
    memory::{ProcessHeap, ThreadStack, PROC_KERNEL_STACK_TOP, PROC_USER_STACK_TOP},
    proc::{Process, Thread, ThreadState},
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
        };

        drop(pt);

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

    /// Finds a thread to run and runs it
    pub fn schedule(&self) -> ! {
        unsafe {
            core::arch::asm!("cli");
        }
        loop {
            let mut guard = self.task_queue.lock();

            let thread: Option<ProcThreadInfo> = guard.pop_front();

            let per_cpu = get_per_cpu();
            if let (false, Some(pid), Some(tid)) = (
                per_cpu.currently_running,
                per_cpu.runing_pid,
                per_cpu.running_tid,
            ) {
                if let Some(thread) = self.get_thread(tid) {
                    if thread.pid == pid {
                        guard.push_back(thread);
                    }
                }
            }

            drop(guard);

            if let Some(thread) = thread {
                let mut lock = thread.thread.running_cpu.lock();
                *lock = Some(core_id());
                // Guard is not dropped here, it will be dropped when an interrupt interrupts this thread
                core::mem::forget(lock);

                thread.thread.jmp_to_userland();
            }

            // If there are no threads to run, sleep
            // We don't want to use a single spin loop hint,
            // otherwise the outer loop continues iterating real fast potentially not letting other cores access to the task_queue guard
            let i = 10000;
            for _ in 0..i {
                core::hint::spin_loop();
            }
        }
    }

    pub fn get_thread_settings(&self) -> SchedulerThreadSettings {
        let guard = self.thread_settings.lock();
        let value = (*guard).clone();
        drop(guard);
        value
    }
}

pub struct CreateProcessOptions {
    pub name: String,
    pub cmdline: String,
    pub cwd: String,

    pub page_table: PageTable,

    pub main_thread_state: ThreadState,
}

pub static SCHEDULER: Scheduler = Scheduler::new();
