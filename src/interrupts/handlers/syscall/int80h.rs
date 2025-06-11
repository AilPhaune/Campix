use core::panic;

use crate::{
    interrupts::{
        handlers::syscall::linux::{linux_syscall, linux_syscall_fast},
        idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    },
    percpu::{get_per_cpu, InterruptSource},
    println,
    process::scheduler::ProcessSyscallABI,
};

pub fn handler(
    ist: u64,
    rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    mut ife: Option<&mut InterruptFrameExtra>,
) {
    let per_cpu = get_per_cpu();
    per_cpu.ensure_enough_allocated_buffers(16);

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

    if let (Some(thread), None | Some(InterruptSource::User)) =
        (&per_cpu.running_thread, per_cpu.interrupt_sources.last())
    {
        let lock = thread.thread.process.syscalls.lock();
        let abi: ProcessSyscallABI = *lock;
        drop(lock);

        let ok = match &mut ife {
            Some(ife) => match abi {
                ProcessSyscallABI::Linux => linux_syscall(ifr, ifc, Some(ife), thread),
            },
            None => match abi {
                ProcessSyscallABI::Linux => linux_syscall(ifr, ifc, None, thread),
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

    print_info!();
    println!("Interrupt 0x80 dump complete.");
    ifr.rax = 0xFFFF_FFFF_FFFF_FFFFu64;
}

pub fn handler_fast() {
    let per_cpu = get_per_cpu();
    per_cpu.ensure_enough_allocated_buffers(16);
    per_cpu.interrupt_sources.push(InterruptSource::Syscall);

    if let Some(thread) = &per_cpu.running_thread {
        let lock = thread.thread.process.syscalls.lock();
        let abi: ProcessSyscallABI = *lock;
        drop(lock);

        unsafe {
            thread.thread.running_cpu.force_unlock();

            let mut lock = thread.thread.running_cpu.lock();
            *lock = None;
            drop(lock);
        }

        match abi {
            ProcessSyscallABI::Linux => linux_syscall_fast(thread),
        };

        return;
    }
    panic!("Bad interrupt.");
}
