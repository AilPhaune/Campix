use crate::debuggable_bitset_enum;

debuggable_bitset_enum!(
    u64,
    pub enum RFlag {
        CarryFlag = 1 << 0,
        ParityFlag = 1 << 2,
        AdjustFlag = 1 << 4,
        ZeroFlag = 1 << 6,
        SignFlag = 1 << 7,
        TrapFlag = 1 << 8,
        InterruptFlag = 1 << 9,
        DirectionFlag = 1 << 10,
        OverflowFlag = 1 << 11,
        IOPL0 = 0 << 12,
        IOPL1 = 1 << 12,
        IOPL2 = 2 << 12,
        IOPL3 = 3 << 12,
        NestedTaskFlag = 1 << 14,
        ResumeFlag = 1 << 16,
        Vm8086Flag = 1 << 17,
        AlignmentCheckFlag = 1 << 18,
        VirtualInterruptFlag = 1 << 19,
        VirtualInterruptPendingFlag = 1 << 20,
        IdentificationFlag = 1 << 21,
    },
    RFlags
);

impl RFlags {
    pub fn read_raw() -> u64 {
        let rflags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {rflags}",
                rflags = out(reg) rflags,
            );
        }
        rflags
    }

    /// # Safety
    /// Modifies the rflags register <br>
    /// If system flags are modified, make sure the code is running in ring 0
    pub unsafe fn write_raw(rflags: u64) {
        core::arch::asm!("push {}", "popfq", in(reg) rflags);
    }

    pub fn read() -> RFlags {
        RFlags::from(Self::read_raw())
    }
}
