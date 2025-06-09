use core::mem::offset_of;

use alloc::vec::Vec;

use crate::data::regs::fs_gs_base::{GsBase, KernelGsBase};

#[derive(Default, Debug, Clone)]
pub struct SyscallData {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

impl SyscallData {
    pub const fn new() -> Self {
        SyscallData {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rdi: 0,
            rsi: 0,
            rsp: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct PerCpu {
    pub exists: bool,
    pub core_id: u8,
    pub interrupted_from_userland: Vec<bool>,
    pub running_pid: Option<u32>,
    pub running_tid: Option<u32>,
    pub syscall_data: SyscallData,
    pub kernel_rsp: u64,
}

impl PerCpu {
    pub const fn new() -> Self {
        PerCpu {
            exists: false,
            core_id: 0,
            interrupted_from_userland: Vec::new(),
            running_pid: None,
            running_tid: None,
            syscall_data: SyscallData::new(),
            kernel_rsp: 0,
        }
    }
}

static mut PER_CPU: [PerCpu; 256] = [const { PerCpu::new() }; 256];

pub fn init_per_cpu(core_id: u8) {
    unsafe {
        PER_CPU[core_id as usize] = PerCpu {
            exists: true,
            core_id,
            interrupted_from_userland: Vec::new(),
            running_pid: None,
            running_tid: None,
            syscall_data: SyscallData::new(),
            kernel_rsp: 0,
        };

        KernelGsBase::set(&PER_CPU[core_id as usize] as *const _ as u64);
        GsBase::use_kernel_base();
    }
}

pub fn core_id() -> u8 {
    let id: u8;
    unsafe {
        core::arch::asm!("mov {id}, gs:[{off}]", id = out(reg_byte) id, off = const offset_of!(PerCpu, core_id));
    }
    id
}

pub fn get_per_cpu() -> &'static mut PerCpu {
    unsafe { &mut PER_CPU[core_id() as usize] }
}
