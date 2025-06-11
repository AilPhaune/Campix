use core::mem::offset_of;

use alloc::{boxed::Box, fmt, format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    data::regs::fs_gs_base::{FsBase, GsBase},
    gdt::{USERLAND_CODE64_SELECTOR, USERLAND_DATA64_SELECTOR},
    paging::PageTable,
    percpu::get_per_cpu,
    process::{io::context::ProcessIOContext, task::get_tss_ref, ui::context::UiContext},
};

use super::{
    memory::{ProcessHeap, ThreadStack},
    scheduler::ProcessSyscallABI,
};

pub struct ProcessAllocatedCode {
    pub allocs: Vec<(u64, Box<[u8]>)>,
}

impl ProcessAllocatedCode {
    pub fn free(&mut self, pt: &mut PageTable) {
        for alloc in self.allocs.iter() {
            unsafe { pt.unmap_4kb(alloc.0, true) };
        }
        self.allocs.clear();
    }
}

impl fmt::Debug for ProcessAllocatedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessAllocatedCode")
            .field("allocs", &format!("[...] - {} elements", self.allocs.len()))
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ProcessAccess {
    pub euid: u32,
    pub egid: u32,
    pub supplementary_gids: Vec<u32>,
}

#[derive(Debug)]
pub enum TaskState {
    Init,
    Running,
    Paused,
    Zombie { exit_code: u64 },
    Dead,
}

#[derive(Debug)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub cwd: Mutex<String>,

    pub uid: u32,
    pub gid: u32,

    pub effective_process_access: Mutex<ProcessAccess>,

    pub page_table: Mutex<PageTable>,
    pub pml4: u64,
    pub heap: Mutex<ProcessHeap>,

    pub threads: Mutex<Vec<Arc<Thread>>>,
    pub zombie_threads: Mutex<Vec<Arc<Thread>>>,

    pub allocated_code: Mutex<ProcessAllocatedCode>,
    pub syscalls: Mutex<ProcessSyscallABI>,

    pub state: Mutex<TaskState>,

    pub io_context: Mutex<ProcessIOContext>,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone)]
pub struct ThreadGPRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

#[derive(Debug, Clone)]
pub struct ThreadState {
    pub gpregs: ThreadGPRegisters,

    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rflags: u64,

    pub fs_base: u64,
    pub gs_base: u64,
}

#[derive(Debug)]
pub struct Thread {
    pub pid: u32,
    pub tid: u32,
    pub process: Arc<Process>,
    pub name: String,

    pub stack: Mutex<ThreadStack>,
    pub kernel_stack: Mutex<ThreadStack>,

    pub state: Mutex<ThreadState>,

    pub running_cpu: Mutex<Option<u8>>,

    pub task_state: Mutex<TaskState>,

    pub ui_context: Mutex<UiContext>,
}

impl Thread {
    pub fn get_running_cpu(&self) -> Option<u8> {
        let guard = self.running_cpu.lock();
        let value = *guard;
        drop(guard);
        value
    }

    pub fn set_running_cpu(&self, cpu: Option<u8>) {
        let mut guard = self.running_cpu.lock();
        *guard = cpu;
        drop(guard);
    }

    fn setup_tss_for_thread(&self) -> u64 {
        let tss = get_tss_ref();

        let kstack = self.kernel_stack.lock();
        tss.rsp0 = kstack.stack_top;
        drop(kstack);

        tss.rsp0
    }

    pub fn jmp_to_userland(&self) -> ! {
        let pml4 = self.process.pml4;

        let kstack = self.setup_tss_for_thread();

        let per_cpu = get_per_cpu();

        per_cpu.kernel_rsp = kstack;
        per_cpu.interrupt_sources.clear();

        unsafe {
            let state = self.state.lock();

            let regs_ptr = &state.gpregs as *const _;

            core::arch::asm!("swapgs");

            FsBase::set(state.fs_base);
            GsBase::set(state.gs_base);

            let rsp = state.rsp;
            let rip = state.rip;
            let rflags = state.rflags;
            let rbp = state.rbp;

            drop(state);

            core::arch::asm!(
                // disable interrupts
                "cli",

                // use process memory
                "mov cr3, r9",

                // setup segment registers
                "mov ds, cx",
                "mov es, cx",
                "mov fs, cx",

                // setup interrupt return
                "push rcx", // user data selector
                "push rdx", // user stack pointer
                "push r11", // user rflags
                "push {user_code_sel}", // user code selector
                "push r8", // user rip

                "mov rbp, r10",

                // restore register values
                "mov r15, [rax + {offset_r15}]",
                "mov r14, [rax + {offset_r14}]",
                "mov r13, [rax + {offset_r13}]",
                "mov r12, [rax + {offset_r12}]",
                "mov r11, [rax + {offset_r11}]",
                "mov r10, [rax + {offset_r10}]",
                "mov r9, [rax + {offset_r9}]",
                "mov r8, [rax + {offset_r8}]",
                "mov rdi, [rax + {offset_rdi}]",
                "mov rsi, [rax + {offset_rsi}]",
                "mov rdx, [rax + {offset_rdx}]",
                "mov rcx, [rax + {offset_rcx}]",
                "mov rbx, [rax + {offset_rbx}]",
                "mov rax, [rax + {offset_rax}]",

                // return
                "iretq",

                // arguments
                in("rax") regs_ptr,
                in("rcx") (USERLAND_DATA64_SELECTOR | 3) as u64,
                in("rdx") rsp,
                in("r8") rip,
                in("r9") pml4,
                in("r10") rbp,
                in("r11") rflags,

                // constants
                user_code_sel = const (USERLAND_CODE64_SELECTOR | 3) as u64,

                offset_rax = const offset_of!(ThreadGPRegisters, rax),
                offset_rbx = const offset_of!(ThreadGPRegisters, rbx),
                offset_rcx = const offset_of!(ThreadGPRegisters, rcx),
                offset_rdx = const offset_of!(ThreadGPRegisters, rdx),
                offset_rsi = const offset_of!(ThreadGPRegisters, rsi),
                offset_rdi = const offset_of!(ThreadGPRegisters, rdi),
                offset_r8 = const offset_of!(ThreadGPRegisters, r8),
                offset_r9 = const offset_of!(ThreadGPRegisters, r9),
                offset_r10 = const offset_of!(ThreadGPRegisters, r10),
                offset_r11 = const offset_of!(ThreadGPRegisters, r11),
                offset_r12 = const offset_of!(ThreadGPRegisters, r12),
                offset_r13 = const offset_of!(ThreadGPRegisters, r13),
                offset_r14 = const offset_of!(ThreadGPRegisters, r14),
                offset_r15 = const offset_of!(ThreadGPRegisters, r15),

                options(noreturn)
            );
        }
    }
}
