use crate::{
    data::regs::{
        msr::{
            efer::{NO_EXECUTE_ENABLE, SYSTEM_CALL_EXTENSION},
            rdmsr, wrmsr, IA32_EFER, LSTAR, SFMASK, STAR,
        },
        rflags::{RFlag, RFlags},
    },
    gdt::{KERNEL_CODE_SELECTOR, USERLAND_CODE64_SELECTOR},
    interrupts::idt::fast_syscall_entry,
};

pub fn init() {
    unsafe {
        // Enable SCE and NXE bit in EFER
        let mut efer = rdmsr(IA32_EFER);
        efer |= SYSTEM_CALL_EXTENSION | NO_EXECUTE_ENABLE;
        wrmsr(IA32_EFER, efer);

        // Setup STAR MSR
        const STAR_VALUE: u64 =
            ((KERNEL_CODE_SELECTOR as u64) << 32) | ((USERLAND_CODE64_SELECTOR as u64 | 3) << 48);
        wrmsr(STAR, STAR_VALUE);

        // Setup LSTAR MSR
        wrmsr(LSTAR, fast_syscall_entry as usize as u64);

        // Setup SFMASK MSR
        const SFMASK_VALUE: u64 = RFlags::empty().set(RFlag::InterruptFlag).get();
        wrmsr(SFMASK, SFMASK_VALUE);
    }
}
