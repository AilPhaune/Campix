use core::panic;

use crate::{
    interrupts::{
        handlers::syscall::linux::{linux_syscall, linux_syscall_fast},
        idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    },
    percpu::get_per_cpu,
    println,
    process::scheduler::{ProcessSyscallABI, SCHEDULER},
};

pub fn handler(
    ist: u64,
    rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    mut ife: Option<&mut InterruptFrameExtra>,
) {
    let per_cpu = get_per_cpu();

    macro_rules! print_info {
        () => {
            println!("Software interrupt 0x80.");
            println!("ist={:#02x}, rsp={:#016x}", ist, rsp);
            println!("{:#?}", per_cpu);
            println!("{:#?}", ifr);
            println!("{:#?}", ifc);
            println!("{:#?}", ife);
        };
    }

    if let (Some(_), Some(tid), None | Some(true)) = (
        per_cpu.running_pid,
        per_cpu.running_tid,
        per_cpu.interrupted_from_userland.last(),
    ) {
        if let Some(mut thread) = SCHEDULER.get_thread(tid) {
            let lock = thread.thread.process.syscalls.lock();
            let abi: ProcessSyscallABI = *lock;
            drop(lock);

            let ok = match &mut ife {
                Some(ife) => match abi {
                    ProcessSyscallABI::Linux => linux_syscall(ifr, ifc, Some(ife), &mut thread),
                },
                None => match abi {
                    ProcessSyscallABI::Linux => linux_syscall(ifr, ifc, None, &mut thread),
                },
            };

            if ok {
                return;
            } else {
                print_info!();
                println!("Interrupt 0x80 dump complete.");
                return;
            }
        }
    }

    print_info!();
    println!("Interrupt 0x80 dump complete.");
    ifr.rax = 0xFFFF_FFFF_FFFF_FFFFu64;
}

pub fn handler_fast() {
    let per_cpu = get_per_cpu();

    if let (Some(_), Some(tid), None | Some(true)) = (
        per_cpu.running_pid,
        per_cpu.running_tid,
        per_cpu.interrupted_from_userland.last(),
    ) {
        if let Some(mut thread) = SCHEDULER.get_thread(tid) {
            let lock = thread.thread.process.syscalls.lock();
            let abi: ProcessSyscallABI = *lock;
            drop(lock);

            match abi {
                ProcessSyscallABI::Linux => linux_syscall_fast(&mut thread),
            };

            return;
        }
    }
    panic!("Bad interrupt.");
}
