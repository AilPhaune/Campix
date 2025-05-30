pub struct Cr2;

impl Cr2 {
    /// # Safety
    /// Caller must ensure the code is running in ring 0 <br>
    /// Reads the value of the CR2 register
    pub unsafe fn read() -> u64 {
        let mut cr2: u64;
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(readonly, nostack, preserves_flags));
        cr2
    }

    /// # Safety
    /// Caller must ensure the code is running in ring 0 <br>
    /// Modifies the value of the CR2 register
    pub unsafe fn write(cr2: u64) {
        core::arch::asm!("mov cr2, {}", in(reg) cr2, options(nostack, preserves_flags));
    }
}

pub struct Cr3;

impl Cr3 {
    /// # Safety
    /// Caller must ensure the code is running in ring 0 <br>
    /// Reads the value of the CR3 register
    pub unsafe fn read() -> u64 {
        let mut cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(readonly, nostack, preserves_flags));
        cr3
    }

    /// # Safety
    /// Caller must ensure the code is running in ring 0 <br>
    /// Modifies the value of the CR3 register
    pub unsafe fn write(cr3: u64) {
        core::arch::asm!("mov cr2, {}", in(reg) cr3, options(nostack, preserves_flags))
    }
}
