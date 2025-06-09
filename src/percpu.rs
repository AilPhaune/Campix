use core::mem::offset_of;

use alloc::vec::Vec;

use crate::data::regs::fs_gs_base::{GsBase, KernelGsBase};

#[derive(Default, Debug, Clone)]
pub struct PerCpu {
    pub exists: bool,
    pub core_id: u8,
    pub interrupted_from_userland: Vec<bool>,
    pub running_pid: Option<u32>,
    pub running_tid: Option<u32>,
}

impl PerCpu {
    pub const fn new() -> Self {
        PerCpu {
            exists: false,
            core_id: 0,
            interrupted_from_userland: Vec::new(),
            running_pid: None,
            running_tid: None,
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
