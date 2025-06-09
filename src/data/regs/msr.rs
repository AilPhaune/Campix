pub const IA32_EFER: u32 = 0xC0000080;
pub const STAR: u32 = 0xC0000081;
pub const LSTAR: u32 = 0xC0000082;
pub const CSTAR: u32 = 0xC0000083;
pub const SFMASK: u32 = 0xC0000084;

pub mod efer {
    pub const SYSTEM_CALL_EXTENSION: u64 = 1 << 0;
    pub const LONG_MODE_ENABLE: u64 = 1 << 8;
    pub const LONG_MODE_ACTIVE: u64 = 1 << 10;
    pub const NO_EXECUTE_ENABLE: u64 = 1 << 11;
    pub const SECURE_VIRTUAL_MACHINE_EXTENSION: u64 = 1 << 12;
    pub const LONG_MODE_SEGMENT_LIMIT_ENABLE: u64 = 1 << 13;
    pub const FFXSR: u64 = 1 << 14;
    pub const TRANSLATION_CACHE_EXTENSION: u64 = 1 << 15;
}

/// # Safety
/// Modifies a model specific register, caller must ensure the code is running in ring 0
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!("wrmsr", in("ecx") msr, in("eax") low, in("edx") high, options(nomem, nostack, preserves_flags));
}

/// # Safety
/// Modifies a model specific register, caller must ensure the code is running in ring 0
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let eax: u32;
    let edx: u32;
    core::arch::asm!("rdmsr", in("ecx") msr, out("eax") eax, out("edx") edx, options(nostack, preserves_flags));
    (eax as u64) | ((edx as u64) << 32)
}
