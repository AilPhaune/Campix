use core::mem::offset_of;

use alloc::{boxed::Box, fmt, format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    data::regs::fs_gs_base::{FsBase, GsBase},
    gdt::{USERLAND_CODE64_SELECTOR, USERLAND_DATA64_SELECTOR},
    paging::PageTable,
    percpu::get_per_cpu,
};

use super::{
    memory::{ProcessHeap, ThreadStack},
    task::{get_tss, set_tss},
};

pub struct ProcessAllocatedCode {
    pub allocs: Vec<Box<[u8]>>,
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

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub name: Arc<String>,
    pub cmdline: Arc<String>,
    pub cwd: Arc<Mutex<String>>,

    pub uid: u32,
    pub gid: u32,

    pub effective_process_access: Arc<Mutex<ProcessAccess>>,

    pub page_table: Arc<Mutex<PageTable>>,
    pub heap: Arc<Mutex<ProcessHeap>>,

    pub allocated_code: Arc<Mutex<ProcessAllocatedCode>>,
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

#[derive(Debug, Clone)]
pub struct Thread {
    pub pid: u32,
    pub tid: u32,
    pub process: Process,
    pub name: Arc<String>,

    pub stack: Arc<Mutex<ThreadStack>>,
    pub kernel_stack: Arc<Mutex<ThreadStack>>,

    pub state: Arc<Mutex<ThreadState>>,

    pub running_cpu: Arc<Mutex<Option<u8>>>,
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

    fn setup_tss_for_thread(&self) {
        let mut tss = get_tss();

        let kstack = self.kernel_stack.lock();
        tss.rsp0 = kstack.stack_top;
        drop(kstack);

        set_tss(&tss);
    }

    pub fn jmp_to_userland(&self) -> ! {
        let pt = self.process.page_table.lock();
        let pml4 = pt.get_pml4();
        drop(pt);

        self.setup_tss_for_thread();

        let per_cpu = get_per_cpu();
        per_cpu.runing_pid = Some(self.pid);
        per_cpu.running_tid = Some(self.tid);

        let state = self.state.lock();

        unsafe {
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
