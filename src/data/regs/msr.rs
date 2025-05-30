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
