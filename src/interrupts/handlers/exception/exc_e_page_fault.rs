use crate::{
    data::regs::cr::Cr2,
    interrupts::idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    paging::{PAGE_ACCESSED, PAGE_PRESENT, PAGE_RW, PAGE_SIZE, PAGE_USER},
    percpu::get_per_cpu,
    printf, println,
    process::{
        memory::{
            get_address_space, HigherHalfAddressSpace, LowerHalfAddressSpace, VirtualAddressSpace,
            PROC_KERNEL_STACK_TOP, PROC_USER_STACK_TOP,
        },
        scheduler::SCHEDULER,
    },
};

const CODE_PRESENT: u64 = 1 << 0;
const CODE_WRITE: u64 = 1 << 1;
const CODE_USER: u64 = 1 << 2;
const CODE_RESERVED_WRITE: u64 = 1 << 3;
const CODE_INSTRUCTION_FETCH: u64 = 1 << 4;
const CODE_PROTECTION_KEY: u64 = 1 << 5;
const CODE_SHADOW_STACK: u64 = 1 << 6;
const CODE_SGX: u64 = 1 << 15;

pub fn handler(
    _interrupt_num: u64,
    rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    ife: Option<&mut InterruptFrameExtra>,
) {
    unsafe {
        let fault_addr = Cr2::read();
        let space = get_address_space(fault_addr);
        let per_cpu = get_per_cpu();

        macro_rules! print_info0 {
            () => {
                println!("Page fault addr={:#016x} in {:?}", fault_addr, space);
                println!("rsp = {:#016x}", rsp);
                println!("{:#?}", ifr);
                println!("{:#?}", ifc);
                println!("{:#?}", ife);
                println!("{:#?}", per_cpu);

                printf!("Error code: {:#b} -- ", ifc.exception_error_code);
                if ifc.exception_error_code & CODE_SGX != 0 {
                    printf!("SGX ");
                }
                if ifc.exception_error_code & CODE_SHADOW_STACK != 0 {
                    printf!("Shadow stack ");
                }
                if ifc.exception_error_code & CODE_PROTECTION_KEY != 0 {
                    printf!("Protection key ");
                }
                if ifc.exception_error_code & CODE_INSTRUCTION_FETCH != 0 {
                    printf!("Instruction fetch ");
                }
                if ifc.exception_error_code & CODE_RESERVED_WRITE != 0 {
                    printf!("Reserved write ");
                }
                if ifc.exception_error_code & CODE_USER == 0 {
                    printf!("Hypervisor ");
                } else {
                    printf!("User ");
                }
                if ifc.exception_error_code & CODE_WRITE == 0 {
                    printf!("Read ");
                } else {
                    printf!("Write ");
                }
                if ifc.exception_error_code & CODE_PRESENT == 0 {
                    printf!("!Present ");
                } else {
                    printf!("Present ");
                }
                println!();
            };
        }

        let Some(tid) = per_cpu.running_tid else {
            print_info0!();
            panic!("Unrecoverable page fault...");
        };

        macro_rules! print_info1 {
            () => {
                print_info0!();
                println!("Running thread id: {}", tid);
            };
        }

        if ifc.exception_error_code & CODE_RESERVED_WRITE != 0
            || ifc.exception_error_code & CODE_PROTECTION_KEY != 0
            || ifc.exception_error_code & CODE_SGX != 0
        {
            print_info1!();
            panic!("Unrecoverable page fault...");
        }

        let Some(thread) = SCHEDULER.get_thread(tid) else {
            print_info1!();
            println!("Running thread not found in scheduler");
            panic!("Unrecoverable page fault...");
        };

        macro_rules! print_info2 {
            () => {
                print_info1!();
                println!("Running process id: {}", thread.pid);
            };
        }

        let tsettings = SCHEDULER.get_thread_settings();

        match space {
            Some(VirtualAddressSpace::HigherHalf(HigherHalfAddressSpace::ProcessKernelStack)) => {
                if ifc.exception_error_code & CODE_USER == 0 {
                    // Only map more kernel stack pages if the fault was in kernel space
                    let n = PROC_KERNEL_STACK_TOP - fault_addr;
                    let npages = n.div_ceil(PAGE_SIZE as u64);

                    if npages > tsettings.max_kernel_stack_pages {
                        print_info2!();
                        println!(
                            "Kernel stack overflow npages={} max={}",
                            npages, tsettings.max_kernel_stack_pages
                        );
                        panic!("Unrecoverable page fault...");
                    }

                    let th = thread.thread;

                    let mut pt = th.process.page_table.lock();
                    let mut kstack = th.kernel_stack.lock();

                    while npages > kstack.stack_buffers.len() as u64 {
                        kstack.grow(&mut pt, PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED);
                    }

                    drop(pt);
                    drop(kstack);

                    return;
                }
            }
            Some(VirtualAddressSpace::LowerHalf(LowerHalfAddressSpace::ProcessStack)) => {
                if ifc.exception_error_code & CODE_USER == CODE_USER {
                    // Only map more kernel stack pages if the fault was in kernel space
                    let n = PROC_USER_STACK_TOP - fault_addr;
                    let npages = n.div_ceil(PAGE_SIZE as u64);

                    if npages > tsettings.max_user_stack_pages {
                        print_info2!();
                        println!(
                            "User stack overflow npages={} max={}",
                            npages, tsettings.max_user_stack_pages
                        );
                        panic!("Unrecoverable page fault...");
                    }

                    let th = thread.thread;

                    let mut pt = th.process.page_table.lock();
                    let mut stack = th.stack.lock();

                    while npages > stack.stack_buffers.len() as u64 {
                        stack.grow(&mut pt, PAGE_PRESENT | PAGE_RW | PAGE_USER | PAGE_ACCESSED);
                    }

                    drop(pt);
                    drop(stack);

                    return;
                }
            }
            _ => (),
        }

        print_info2!();
        panic!("Page fault addr={:#016x} in {:?}", fault_addr, space);
    }
}
