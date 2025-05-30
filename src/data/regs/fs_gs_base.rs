use crate::process::memory::{get_address_space, VirtualAddressSpace};

use super::msr::{rdmsr, wrmsr};

pub const IA32_FS_BASE: u32 = 0xC000_0100;
pub const IA32_GS_BASE: u32 = 0xC000_0101;
pub const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

pub struct FsBase;
pub struct GsBase;
pub struct KernelGsBase;

impl FsBase {
    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn get() -> u64 {
        rdmsr(IA32_FS_BASE)
    }

    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn set(value: u64) {
        wrmsr(IA32_FS_BASE, value)
    }
}

impl GsBase {
    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn get() -> u64 {
        rdmsr(IA32_GS_BASE)
    }

    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn set(value: u64) {
        wrmsr(IA32_GS_BASE, value)
    }

    /// Returns true if gs_base was swapped
    ///
    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn use_kernel_base() -> bool {
        let current_base = Self::get();
        if matches!(
            get_address_space(current_base),
            Some(VirtualAddressSpace::HigherHalf(..))
        ) {
            // already using kernel base
            return false;
        }

        core::arch::asm!("swapgs");
        true
    }

    /// Returns true if gs_base was swapped
    ///
    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn use_user_base() -> bool {
        let current_base = Self::get();
        if matches!(
            get_address_space(current_base),
            Some(VirtualAddressSpace::HigherHalf(..))
        ) {
            // using kernel base
            core::arch::asm!("swapgs");
            return true;
        }
        false
    }
}

impl KernelGsBase {
    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn get() -> u64 {
        rdmsr(IA32_KERNEL_GS_BASE)
    }

    /// # Safety
    /// Caller must ensure the code is running in ring 0
    pub unsafe fn set(value: u64) {
        wrmsr(IA32_KERNEL_GS_BASE, value)
    }
}
