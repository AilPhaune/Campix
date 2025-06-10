#[repr(C, packed(4))]
pub struct RawTaskStateSegment {
    pub reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: [u32; 2],
    pub ist: [u64; 7],
    pub reserved2: [u32; 2],
    pub reserved3: u16,
    pub iopb: u16,
}

#[derive(Debug)]
pub struct TaskStateSegment {
    pub reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    pub reserved1: [u32; 2],
    pub ist: [u64; 7],
    pub reserved2: [u32; 2],
    pub reserved3: u16,
    pub iopb: u16,
}

#[repr(C, align(16))]
pub struct AlignedTSS([u8; size_of::<RawTaskStateSegment>()]);

static mut TSS: AlignedTSS = AlignedTSS([0; size_of::<RawTaskStateSegment>()]);

impl AlignedTSS {
    #[inline(always)]
    pub fn get_tss_addr(&self) -> u64 {
        self.0.as_ptr() as u64
    }
}

#[allow(static_mut_refs)]
#[inline(always)]
pub fn get_tss_addr() -> u64 {
    unsafe { TSS.get_tss_addr() }
}

#[allow(static_mut_refs)]
#[inline(always)]
pub fn get_tss_ref() -> &'static mut RawTaskStateSegment {
    unsafe { &mut *(TSS.get_tss_addr() as *mut RawTaskStateSegment) }
}

#[allow(static_mut_refs)]
pub fn get_tss() -> TaskStateSegment {
    let raw = unsafe { core::ptr::read_volatile(TSS.get_tss_addr() as *mut RawTaskStateSegment) };

    TaskStateSegment {
        reserved0: raw.reserved0,
        rsp0: raw.rsp0,
        rsp1: raw.rsp1,
        rsp2: raw.rsp2,
        reserved1: raw.reserved1,
        ist: raw.ist,
        reserved2: raw.reserved2,
        reserved3: raw.reserved3,
        iopb: raw.iopb,
    }
}

#[allow(static_mut_refs)]
pub fn set_tss(tss: &TaskStateSegment) {
    let raw = RawTaskStateSegment {
        reserved0: tss.reserved0,
        rsp0: tss.rsp0,
        rsp1: tss.rsp1,
        rsp2: tss.rsp2,
        reserved1: tss.reserved1,
        ist: tss.ist,
        reserved2: tss.reserved2,
        reserved3: tss.reserved3,
        iopb: tss.iopb,
    };

    unsafe {
        core::ptr::write_volatile(TSS.get_tss_addr() as *mut RawTaskStateSegment, raw);
    }
}
