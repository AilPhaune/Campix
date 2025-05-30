use alloc::{boxed::Box, vec::Vec};

use crate::{
    data::calloc_boxed_slice,
    paging::{PageTable, DIRECT_MAPPING_OFFSET, PAGE_SIZE},
};

#[derive(Debug, Clone, Copy)]
pub enum VirtualAddressSpace {
    HigherHalf(HigherHalfAddressSpace),
    LowerHalf(LowerHalfAddressSpace),
}

#[derive(Debug, Clone, Copy)]
pub enum HigherHalfAddressSpace {
    GlobalKernelMappedCode,
    GlobalKernelStack,
    GlobalDirectMemoryMapping,
    GlobalMemoryMappedIo,
    ProcessKernelStack,
    None,
}

#[derive(Debug, Clone, Copy)]
pub enum LowerHalfAddressSpace {
    ProcessStack,
    ProcessCode,
    ProcessHeap,
    None,
}

pub const PROC_KERNEL_STACK_TOP: u64 = 0xFFFF_D000_0000_0000;
pub const GLOB_KERNEL_MMIO_TOP: u64 = 0xFFFF_C000_0000_0000;
pub const GLOB_KERNEL_DIRECT_MAPPED_TOP: u64 = DIRECT_MAPPING_OFFSET + 0x0000_1000_0000_0000;
pub const GLOB_KERNEL_STACK_TOP: u64 = 0xFFFF_A000_0000_0000;
pub const GLOB_KERNEL_CODE_TOP: u64 = 0xFFFF_9000_0000_0000;

pub const HIGHER_HALF_BEGIN: u64 = 0xFFFF_8000_0000_0000;
pub const LOWER_HALF_END: u64 = 0x0000_8000_0000_0000;

pub const LOWER_HALF_SAFEGUARD_END: u64 = 0x0000_1000_0000_0000;
pub const PROC_USER_STACK_TOP: u64 = 0x0000_2000_0000_0000;
pub const PROC_MAPPED_CODE_TOP: u64 = 0x0000_3000_0000_0000;
pub const PROC_HEAP_TOP: u64 = 0x0000_4000_0000_0000;

pub const fn get_address_space(addr: u64) -> Option<VirtualAddressSpace> {
    if addr >= HIGHER_HALF_BEGIN {
        if addr < GLOB_KERNEL_CODE_TOP {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::GlobalKernelMappedCode,
            ))
        } else if addr < GLOB_KERNEL_STACK_TOP {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::GlobalKernelStack,
            ))
        } else if addr < GLOB_KERNEL_DIRECT_MAPPED_TOP {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::GlobalDirectMemoryMapping,
            ))
        } else if addr < GLOB_KERNEL_MMIO_TOP {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::GlobalMemoryMappedIo,
            ))
        } else if addr < PROC_KERNEL_STACK_TOP {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::ProcessKernelStack,
            ))
        } else {
            Some(VirtualAddressSpace::HigherHalf(
                HigherHalfAddressSpace::None,
            ))
        }
    } else if addr < LOWER_HALF_END {
        if addr < LOWER_HALF_SAFEGUARD_END {
            Some(VirtualAddressSpace::LowerHalf(LowerHalfAddressSpace::None))
        } else if addr < PROC_USER_STACK_TOP {
            Some(VirtualAddressSpace::LowerHalf(
                LowerHalfAddressSpace::ProcessStack,
            ))
        } else if addr < PROC_MAPPED_CODE_TOP {
            Some(VirtualAddressSpace::LowerHalf(
                LowerHalfAddressSpace::ProcessCode,
            ))
        } else if addr < PROC_HEAP_TOP {
            Some(VirtualAddressSpace::LowerHalf(
                LowerHalfAddressSpace::ProcessHeap,
            ))
        } else {
            Some(VirtualAddressSpace::LowerHalf(LowerHalfAddressSpace::None))
        }
    } else {
        None
    }
}

#[derive(Debug, Default)]
pub struct ProcessHeap {}

impl ProcessHeap {
    pub fn new() -> Self {
        ProcessHeap {}
    }
}

#[derive(Debug)]
pub struct ThreadStack {
    pub stack_top: u64,
    pub stack_size: u64,

    pub stack_buffers: Vec<Box<[u8]>>,
}

impl ThreadStack {
    pub fn new(stack_top: u64) -> Self {
        Self {
            stack_top,
            stack_size: 0,
            stack_buffers: Vec::new(),
        }
    }

    pub fn new_with_pages(
        stack_top: u64,
        num_pages: u64,
        table: &mut PageTable,
        flags: u64,
    ) -> Self {
        let mut stack = Self::new(stack_top);
        for _ in 0..num_pages {
            stack.grow(table, flags);
        }
        stack
    }

    pub fn get_bottom(&self) -> u64 {
        self.stack_top - self.stack_size
    }

    pub fn grow(&mut self, table: &mut PageTable, flags: u64) {
        let new_buffer = calloc_boxed_slice::<u8>(PAGE_SIZE);
        self.stack_size += PAGE_SIZE as u64;

        let kernel_virt = new_buffer.as_ptr() as u64;
        let phys = kernel_virt - DIRECT_MAPPING_OFFSET;
        let proc_virt = self.get_bottom();

        unsafe { table.map_4kb(proc_virt, phys, flags, true) };

        self.stack_buffers.push(new_buffer);
    }
}
