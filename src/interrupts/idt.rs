use core::{
    arch::{asm, naked_asm},
    mem::offset_of,
};

use alloc::boxed::Box;

use crate::{
    data::{calloc_boxed_slice, regs::fs_gs_base::GsBase},
    gdt::{KERNEL_CODE_SELECTOR, KERNEL_DATA_SELECTOR},
    interrupts::pic::pic_send_eoi,
    paging::{get_kernel_page_table, DIRECT_MAPPING_OFFSET, PAGE_ACCESSED, PAGE_PRESENT, PAGE_RW},
    percpu::{core_id, get_per_cpu, InterruptSource, PerCpu},
    println,
    process::{
        memory::GLOB_KERNEL_STACK_TOP,
        task::{get_tss, set_tss},
    },
};

use super::handlers;

pub const IDT_PRESENT: u8 = 1 << 7;
pub const IDT_DPL0: u8 = 0 << 5;
pub const IDT_DPL1: u8 = 1 << 5;
pub const IDT_DPL2: u8 = 2 << 5;
pub const IDT_DPL3: u8 = 3 << 5;
pub const IDT_STORAGE_SEGMENT: u8 = 0 << 4;

pub const IDT_TYPE_TASK_GATE: u8 = 0x5;
pub const IDT_TYPE_16BIT_INT_GATE: u8 = 0x6;
pub const IDT_TYPE_16BIT_TRAP_GATE: u8 = 0x7;
pub const IDT_TYPE_32BIT_INT_GATE: u8 = 0xE;
pub const IDT_TYPE_32BIT_TRAP_GATE: u8 = 0xF;
pub const IDT_TYPE_64BIT_INT_GATE: u8 = IDT_TYPE_32BIT_INT_GATE;

pub const KERNEL_INT_FLAGS: u8 =
    IDT_PRESENT | IDT_DPL0 | IDT_STORAGE_SEGMENT | IDT_TYPE_64BIT_INT_GATE;
pub const USER_INT_FLAGS: u8 =
    IDT_PRESENT | IDT_DPL3 | IDT_STORAGE_SEGMENT | IDT_TYPE_64BIT_INT_GATE;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry64 {
    isr_low: u16,
    kernel_cs: u16,
    ist: u8,
    flags: u8,
    isr_mid: u16,
    isr_high: u32,
    reserved: u32,
}

impl IdtEntry64 {
    const fn missing() -> Self {
        Self {
            isr_low: 0,
            kernel_cs: 0,
            ist: 0,
            flags: 0,
            isr_mid: 0,
            isr_high: 0,
            reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: extern "C" fn(), selector: u16, ist: u8, flags: u8) {
        let addr = handler as usize as u64;
        self.isr_low = (addr & 0xFFFF) as u16;
        self.kernel_cs = selector;
        self.ist = ist;
        self.flags = flags;
        self.isr_mid = ((addr >> 16) & 0xFFFF) as u16;
        self.isr_high = (addr >> 32) as u32;
        self.reserved = 0;
    }
}

#[repr(C, align(16))]
struct Idt {
    entries: [IdtEntry64; 256],
}

static mut IDT: Idt = Idt {
    entries: [IdtEntry64::missing(); 256],
};

#[repr(C, packed)]
#[derive(Debug, Clone)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

#[repr(C, align(16))]
struct AlignedIdtDescriptor([IdtDescriptor; 1]);

impl AlignedIdtDescriptor {
    const fn new() -> Self {
        Self([IdtDescriptor { limit: 0, base: 0 }])
    }
}

static mut IDT_DESCRIPTOR: AlignedIdtDescriptor = AlignedIdtDescriptor::new();

#[allow(static_mut_refs)]
unsafe fn load_idt(idt: &Idt) {
    let descriptor = IdtDescriptor {
        limit: (core::mem::size_of::<Idt>() - 1) as u16,
        base: idt as *const _ as u64,
    };
    println!("Loading IDT.");
    println!("IDT Descriptor: {:#?}", descriptor);
    println!("IDT Descriptor Address: {:?}", IDT_DESCRIPTOR.0.as_ptr());
    core::ptr::write_volatile(IDT_DESCRIPTOR.0.as_ptr() as *mut IdtDescriptor, descriptor);
    asm!(
        "lidt [{}]",
        in(reg) IDT_DESCRIPTOR.0.as_ptr(),
        options(readonly, nostack, preserves_flags)
    );
}

fn unhandled_interrupt(
    int: u64,
    _rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    ife: Option<&mut InterruptFrameExtra>,
) {
    println!("Unhandled interrupt {:#02x}.", int);
    println!("{:#?}", ifr);
    println!("{:#?}", ifc);
    println!("{:#?}", ife);
    panic!("Unhandled interrupt dump complete.");
}

pub type HandlerFnType = fn(
    u64,
    u64,
    &mut InterruptFrameRegisters,
    &mut InterruptFrameContext,
    Option<&mut InterruptFrameExtra>,
);

static mut HANDLERS: [HandlerFnType; 256] = [unhandled_interrupt; 256];

extern "C" {
    static isr_stub_table: [extern "C" fn(); 256];
}

#[no_mangle]
#[unsafe(naked)]
/// # Safety
/// DO NOT CALL
pub unsafe extern "C" fn fast_syscall_entry() {
    naked_asm!(
        "swapgs",
        "mov gs:[{offset_rax}], rax",
        "mov gs:[{offset_rbx}], rbx",
        "mov gs:[{offset_rcx}], rcx",
        "mov gs:[{offset_rdx}], rdx",
        "mov gs:[{offset_rdi}], rdi",
        "mov gs:[{offset_rsi}], rsi",
        "mov gs:[{offset_rsp}], rsp",
        "mov gs:[{offset_rbp}], rbp",
        "mov gs:[{offset_r8}], r8",
        "mov gs:[{offset_r9}], r9",
        "mov gs:[{offset_r10}], r10",
        "mov gs:[{offset_r11}], r11",
        "mov gs:[{offset_r12}], r12",
        "mov gs:[{offset_r13}], r13",
        "mov gs:[{offset_r14}], r14",
        "mov gs:[{offset_r15}], r15",

        "mov rsp, gs:[{offset_kernel_rsp}]",
        "call {fn_syscall_handler}",

        "mov rax, gs:[{offset_rax}]",
        "mov rbx, gs:[{offset_rbx}]",
        "mov rcx, gs:[{offset_rcx}]",
        "mov rdx, gs:[{offset_rdx}]",
        "mov rdi, gs:[{offset_rdi}]",
        "mov rsi, gs:[{offset_rsi}]",
        "mov rsp, gs:[{offset_rsp}]",
        "mov rbp, gs:[{offset_rbp}]",
        "mov r8, gs:[{offset_r8}]",
        "mov r9, gs:[{offset_r9}]",
        "mov r10, gs:[{offset_r10}]",
        "mov r11, gs:[{offset_r11}]",
        "mov r12, gs:[{offset_r12}]",
        "mov r13, gs:[{offset_r13}]",
        "mov r14, gs:[{offset_r14}]",
        "mov r15, gs:[{offset_r15}]",
        "mov rbp, rsp",
        "swapgs",
        "sysretq",

        offset_rax = const offset_of!(PerCpu, syscall_data.rax),
        offset_rbx = const offset_of!(PerCpu, syscall_data.rbx),
        offset_rcx = const offset_of!(PerCpu, syscall_data.rcx),
        offset_rdx = const offset_of!(PerCpu, syscall_data.rdx),
        offset_rdi = const offset_of!(PerCpu, syscall_data.rdi),
        offset_rsi = const offset_of!(PerCpu, syscall_data.rsi),
        offset_rsp = const offset_of!(PerCpu, syscall_data.rsp),
        offset_rbp = const offset_of!(PerCpu, syscall_data.rbp),
        offset_r8 = const offset_of!(PerCpu, syscall_data.r8),
        offset_r9 = const offset_of!(PerCpu, syscall_data.r9),
        offset_r10 = const offset_of!(PerCpu, syscall_data.r10),
        offset_r11 = const offset_of!(PerCpu, syscall_data.r11),
        offset_r12 = const offset_of!(PerCpu, syscall_data.r12),
        offset_r13 = const offset_of!(PerCpu, syscall_data.r13),
        offset_r14 = const offset_of!(PerCpu, syscall_data.r14),
        offset_r15 = const offset_of!(PerCpu, syscall_data.r15),

        offset_kernel_rsp = const offset_of!(PerCpu, kernel_rsp),
        fn_syscall_handler = sym handlers::syscall::int80h::handler_fast,
    );
}

#[no_mangle]
pub extern "C" fn idt_exception_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        let swap = GsBase::use_kernel_base();

        let (ifr, ifc, ife) = common_enter_interrupt(rsp);

        if let Some(ife) = ife {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, Some(ife));

            common_exit_interrupt(ifr, ifc, Some(ife));
        } else {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, None);

            common_exit_interrupt(ifr, ifc, None);
        }

        if swap {
            core::arch::asm!("swapgs");
        }
    }
}

#[no_mangle]
pub extern "C" fn idt_irq_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        let swap = GsBase::use_kernel_base();

        let (ifr, ifc, ife) = common_enter_interrupt(rsp);

        if let Some(ife) = ife {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, Some(ife));

            common_exit_interrupt(ifr, ifc, Some(ife));
        } else {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, None);

            common_exit_interrupt(ifr, ifc, None);
        }

        if swap {
            core::arch::asm!("swapgs");
        }
    }

    pic_send_eoi(interrupt_num as u8 - 32);
}

#[no_mangle]
pub extern "C" fn idt_software_interrupt_handler(interrupt_num: u64, rsp: u64) {
    unsafe {
        let mut ds: u16;
        let mut es: u16;
        let mut ss: u16;

        asm!(
            "mov {ds:x}, ds",
            "mov {es:x}, es",
            "mov {ss:x}, ss",
            ds = out(reg) ds,
            es = out(reg) es,
            ss = out(reg) ss,
            options(readonly, nostack, preserves_flags)
        );

        asm!(
            "mov ds, {data_seg:x}",
            "mov es, {data_seg:x}",
            "mov ss, {data_seg:x}",
            data_seg = in(reg) KERNEL_DATA_SELECTOR,
            options(readonly, nostack, preserves_flags)
        );

        let swap = GsBase::use_kernel_base();

        let (ifr, ifc, ife) = common_enter_interrupt(rsp);

        if let Some(ife) = ife {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, Some(ife));

            common_exit_interrupt(ifr, ifc, Some(ife));
        } else {
            HANDLERS[interrupt_num as usize](interrupt_num, rsp, ifr, ifc, None);

            common_exit_interrupt(ifr, ifc, None);
        }

        if swap {
            core::arch::asm!("swapgs");
        }

        asm!(
            "mov ds, {ds:x}",
            "mov es, {es:x}",
            "mov ss, {ss:x}",
            ds = in(reg) ds,
            es = in(reg) es,
            ss = in(reg) ss,
            options(readonly, nostack, preserves_flags)
        );
    }
}

struct IstStack {
    data: Box<[u64]>,
    mapped_virt: u64,
    mapped_virt_top: u64,
}

static mut IST_STACKS: [Option<IstStack>; 7] = [const { None }; 7];
const STACK_SEPARATION: u64 = 8 * 1024 * 1024 * 1024; // 8GiB

pub fn init_interrupts() {
    unsafe {
        for (i, f) in isr_stub_table.iter().enumerate() {
            IDT.entries[i].set_handler(*f, KERNEL_CODE_SELECTOR as u16, 0, KERNEL_INT_FLAGS);
        }
        IDT.entries[0x80].flags = USER_INT_FLAGS;

        let mut curr_ist_top = GLOB_KERNEL_STACK_TOP - STACK_SEPARATION;
        let mut kpages = get_kernel_page_table().lock();
        let mut tss = get_tss();
        #[allow(static_mut_refs)]
        for (i, stack) in IST_STACKS.iter_mut().enumerate() {
            // Allocate 2Mb stack
            let ist = IstStack {
                data: calloc_boxed_slice(2 * 1024 * 1024),
                mapped_virt: curr_ist_top,
                mapped_virt_top: curr_ist_top + 2 * 1024 * 1024,
            };

            kpages.map_2mb(
                ist.mapped_virt,
                ist.data.as_ptr() as u64 - DIRECT_MAPPING_OFFSET,
                PAGE_RW | PAGE_ACCESSED | PAGE_PRESENT,
                true,
            );

            curr_ist_top -= STACK_SEPARATION;
            tss.ist[i] = ist.mapped_virt_top;
            *stack = Some(ist);
        }
        set_tss(&tss);

        IDT.entries[0x0E].ist = 1;
        IDT.entries[0x08].ist = 2;

        HANDLERS[0x20] = handlers::irq::irq0_timer::handler;
        HANDLERS[0x21] = handlers::irq::irq1_keyboard::handler;

        HANDLERS[0x06] = handlers::exception::exc_6_invalid_opcode::handler;
        HANDLERS[0x0E] = handlers::exception::exc_e_page_fault::handler;

        HANDLERS[0x80] = handlers::syscall::int80h::handler;

        #[allow(static_mut_refs)]
        load_idt(&IDT);
    }
}

#[repr(C, packed(8))]
#[derive(Debug, Clone)]
pub struct InterruptFrameRegisters {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone)]
pub struct InterruptFrameContext {
    pub exception_error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

#[repr(C, packed(8))]
#[derive(Debug, Clone)]
pub struct InterruptFrameExtra {
    pub rsp: u64,
    pub ss: u64,
}

unsafe fn get_interrupt_context(
    rsp: u64,
) -> (
    &'static mut InterruptFrameRegisters,
    &'static mut InterruptFrameContext,
    Option<&'static mut InterruptFrameExtra>,
) {
    let ifr =
        &mut *((rsp - size_of::<InterruptFrameRegisters>() as u64) as *mut InterruptFrameRegisters);
    let ifc = &mut *(rsp as *mut InterruptFrameContext);

    if ifc.cs & 0b11 != 0 {
        // There was a privilege level change so the cpu pushed extra information

        let ife =
            &mut *((rsp + size_of::<InterruptFrameContext>() as u64) as *mut InterruptFrameExtra);

        return (ifr, ifc, Some(ife));
    }

    (ifr, ifc, None)
}

fn common_exit_interrupt(
    _ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
) {
    let per_cpu = get_per_cpu();
    per_cpu.interrupt_sources.pop();

    if ifc.cs & 0b11 != 0 {
        // If the interrupt comes from lower privilege level, we need to lock back the thread
        if let Some(thread) = &per_cpu.running_thread {
            let mut lock = thread.thread.running_cpu.lock();
            *lock = Some(core_id());
            core::mem::forget(lock);
        }
    }
}

fn common_enter_interrupt(
    rsp: u64,
) -> (
    &'static mut InterruptFrameRegisters,
    &'static mut InterruptFrameContext,
    Option<&'static mut InterruptFrameExtra>,
) {
    let per_cpu = get_per_cpu();

    let (ifr, ifc, ife) = unsafe { get_interrupt_context(rsp) };

    per_cpu.interrupt_sources.push(if ifc.cs & 0b11 == 0 {
        InterruptSource::Kernel
    } else {
        InterruptSource::Syscall
    });

    if ifc.cs & 0b11 != 0 {
        if let Some(ife) = &ife {
            // If the interrupt comes from lower privilege level, we need to unlock the thread as it is not running anymore

            if let Some(thread) = &per_cpu.running_thread {
                unsafe {
                    let mut lock = thread.thread.state.lock();
                    lock.rflags = ifc.rflags;
                    lock.rip = ifc.rip;
                    lock.rbp = ifr.rbp;
                    lock.rsp = ife.rsp;
                    lock.gpregs.rax = ifr.rax;
                    lock.gpregs.rbx = ifr.rbx;
                    lock.gpregs.rcx = ifr.rcx;
                    lock.gpregs.rdx = ifr.rdx;
                    lock.gpregs.rdi = ifr.rdi;
                    lock.gpregs.rsi = ifr.rsi;
                    lock.gpregs.r8 = ifr.r8;
                    lock.gpregs.r9 = ifr.r9;
                    lock.gpregs.r10 = ifr.r10;
                    lock.gpregs.r11 = ifr.r11;
                    lock.gpregs.r12 = ifr.r12;
                    lock.gpregs.r13 = ifr.r13;
                    lock.gpregs.r14 = ifr.r14;
                    lock.gpregs.r15 = ifr.r15;
                    drop(lock);

                    thread.thread.running_cpu.force_unlock();

                    let mut lock = thread.thread.running_cpu.lock();
                    *lock = None;
                    drop(lock);
                }
            }
        } else {
            panic!("ifc.cs & 0b11 != 0 but ife == None");
        }
    }

    (ifr, ifc, ife)
}
