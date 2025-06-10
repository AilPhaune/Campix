#![no_std]
#![no_main]
#![feature(unsafe_cell_access)]

use core::num::NonZeroUsize;

use alloc::{boxed::Box, string::ToString, vec::Vec};
use data::file::File;
use drivers::{
    fs::phys::ext2::Ext2Volume,
    pci,
    vfs::{get_vfs, OPEN_MODE_BINARY, OPEN_MODE_READ, OPEN_MODE_WRITE},
};
use memory::mem::OsMemoryRegion;
use obsiboot::ObsiBootKernelParameters;
use paging::{init_paging, physical_to_virtual};
use process::{executable::parse_executable, scheduler::SCHEDULER};

extern crate alloc;

pub mod data;
pub mod drivers;
pub mod e9;
pub mod formats;
pub mod gdt;
pub mod interrupts;
pub mod io;
pub mod memory;
pub mod obsiboot;
pub mod paging;
pub mod percpu;
pub mod process;
pub mod syscalls;
pub mod vesa;

#[no_mangle]
pub fn _start(obsiboot_ptr: u64) -> ! {
    let mut obsiboot =
        unsafe { core::ptr::read_volatile(obsiboot_ptr as *const ObsiBootKernelParameters) };

    unsafe {
        println!("Campix Kernel");
        println!("{:#?}", obsiboot);
        println!();

        if obsiboot.obsiboot_struct_version != 1 {
            let version = obsiboot.obsiboot_struct_version;
            panic!("Unsupported ObsiBoot struct version: {}", version);
        }

        if !obsiboot.verify_checksum() {
            panic!("Invalid ObsiBoot struct checksum");
        }

        init_paging(
            obsiboot.ptr_to_memory_layout as *const OsMemoryRegion,
            obsiboot.memory_layout_entry_count as u64,
            obsiboot.pml4_base_address as u64,
            obsiboot.page_tables_page_allocator_current_free_page as u64,
            obsiboot.page_tables_page_allocator_last_usable_page as u64,
            obsiboot.kernel_stack_pointer,
        );
        println!("Paging initialized");

        gdt::init_gdtr();
        println!("GDT initialized");

        memory::mem::init(
            physical_to_virtual(obsiboot.ptr_to_memory_layout as u64) as *const OsMemoryRegion,
            obsiboot.memory_layout_entry_count as u64,
            obsiboot.pml4_base_address as u64,
            obsiboot.usable_kernel_memory_start as u64,
        );
        println!("Memory allocator initialized");

        percpu::init_per_cpu(0);
        println!("Per-CPU initialized");

        interrupts::init();
        println!("Interrupts initialized");

        syscalls::init();
        println!("Syscalls initialized");

        vesa::parse_current_mode(&obsiboot);
        println!("VESA initialized");

        {
            println!("\nEnumerating PCI devices:");
            let devices = pci::scan_bus();
            for device in devices.iter() {
                println!("{:?}", device);
            }
        }

        {
            let file = File::open(
                "/dev/pata_pm_p0",
                OPEN_MODE_READ | OPEN_MODE_WRITE | OPEN_MODE_BINARY,
            )
            .unwrap();
            let ext2 = Ext2Volume::from_device(
                file,
                NonZeroUsize::new(1024 * 1024).unwrap(),
                NonZeroUsize::new(1024 * 1024).unwrap(),
                NonZeroUsize::new(1024 * 1024).unwrap(),
            )
            .unwrap();

            let vfs = get_vfs();
            let mut wguard = vfs.write();
            wguard
                .mount(&"system".chars().collect::<Vec<char>>(), Box::new(ext2))
                .unwrap();
            drop(wguard);
        }

        kmain(obsiboot);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        _handle_panic(info);
        core::arch::asm!("cli", "hlt");
    }
    loop {}
}

#[no_mangle]
unsafe fn _handle_panic(info: &core::panic::PanicInfo) {
    printf!("Panic: {}\n", info.message());

    match info.location() {
        Some(loc) => {
            printf!("Location: {}\n", loc);
        }
        None => printf!("Location unknown !\n"),
    }
}

unsafe fn kmain(obsiboot: ObsiBootKernelParameters) -> ! {
    let mode = vesa::get_mode_info();

    println!("Kernel display using vesa mode {:#?}", mode);
    println!("Available modes:");
    for (mode, info) in vesa::iter_modes(&obsiboot) {
        let vesa::VesaModeInfoStructure {
            width, height, bpp, ..
        } = info;
        println!("{}: {}x{}:{}bpp", mode, width, height, bpp);
    }
    println!();

    let stats = match File::get_stats("/system/sysinit") {
        Ok(Some(stats)) => stats,
        Ok(None) => {
            println!("Initial executable /system/sysinit not found, make sure it exists in the system partition, then reboot.");
            println!();
            panic!("Campix: failed to boot...");
        }
        Err(err) => {
            println!("Could not get stats for /system/sysinit");
            println!("Error: {:#?}", err);
            println!();
            panic!("Campix: failed to boot...");
        }
    };

    if !stats.is_file {
        println!("Initial executable /system/sysinit is not a file, make sure it exists in the system partition and that it is not a symlink.");
        println!();
        panic!("Campix: failed to boot...");
    }

    let executable = match parse_executable("/system/sysinit") {
        Ok(executable) => executable,
        Err(err) => {
            println!("Could not parse /system/sysinit");
            println!("Errors: {:#?}", err);
            println!();
            panic!("Campix: failed to boot...");
        }
    };

    let options = match executable.create_process(
        "sysinit".to_string(),
        "/system/sysinit".to_string(),
        "/".to_string(),
        0,
        0,
        alloc::vec![],
    ) {
        Ok(options) => options,
        Err(err) => {
            println!("Could not create process /system/sysinit");
            println!("Error: {:#?}", err);
            println!();
            panic!("Campix: failed to boot...");
        }
    };

    SCHEDULER.create_process(options);
    SCHEDULER.schedule();
}
