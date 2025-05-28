use crate::interrupts::idt::Registers;

#[repr(C, packed)]
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

pub static mut TSS: TaskStateSegment = TaskStateSegment {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: [0; 2],
    ist: [0; 7],
    reserved2: [0; 2],
    reserved3: 0,
    iopb: 0,
};

pub struct ThreadControlBlock {
    pub pid: u64,
    pub id: u64,

    pub kernel_stack: u64,
    pub user_stack: u64,

    pub cr3_phys: u64,

    pub registers: Registers,
}
