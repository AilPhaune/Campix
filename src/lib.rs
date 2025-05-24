#![no_std]
#![no_main]

use core::num::NonZeroUsize;

use alloc::{boxed::Box, string::String, vec::Vec};
use data::file::{DirectoryEntry, File};
use drivers::{
    fs::phys::ext2::Ext2Volume,
    pci,
    vfs::{get_vfs, OPEN_MODE_BINARY, OPEN_MODE_READ, OPEN_MODE_WRITE},
};
use memory::mem::OsMemoryRegion;
use obsiboot::ObsiBootKernelParameters;
use paging::{init_paging, physical_to_virtual};

extern crate alloc;

pub mod data;
pub mod drivers;
pub mod e9;
pub mod gdt;
pub mod interrupts;
pub mod io;
pub mod memory;
pub mod obsiboot;
pub mod paging;
pub mod vesa;

#[no_mangle]
pub fn _start(obsiboot_ptr: u64) -> ! {
    let mut obsiboot =
        unsafe { (obsiboot_ptr as *const ObsiBootKernelParameters).read_unaligned() };

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
        );
        println!("Paging initialized");

        gdt::init_gdtr();
        println!("GDT initialized");

        interrupts::init();
        println!("Interrupts initialized");

        memory::mem::init(
            physical_to_virtual(obsiboot.ptr_to_memory_layout as u64) as *const OsMemoryRegion,
            obsiboot.memory_layout_entry_count as u64,
            obsiboot.pml4_base_address as u64,
            obsiboot.usable_kernel_memory_start as u64,
        );
        println!("Memory allocator initialized");

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

            println!("\nListing files:");
            let directory = DirectoryEntry::of("/").unwrap();
            dumpfs_tree(&directory, 0);
            println!();
        }

        {
            File::delete("/system/aaa").unwrap();
            File::delete("/system/lost+found").unwrap();
            File::delete("/system/obsiboot.conf").unwrap();
            // DESTROY SYSTEM BECAUSE IT'S FUN
            File::delete("/system/kernel64.elf").unwrap();
        }

        {
            let vfs = get_vfs();
            let mut wguard = vfs.write();
            wguard
                .unmount(&"system".chars().collect::<Vec<char>>())
                .unwrap();
        }

        kmain(obsiboot);
    }
}

pub fn dumpfs_tree(dir: &DirectoryEntry, indent: usize) {
    let name = dir.name();
    let name = name.iter().collect::<String>();
    println!("{}/{}", " ".repeat(indent), name);
    if let Ok(dir) = dir.get_dir() {
        for entry in dir.list().unwrap().iter() {
            dumpfs_tree(entry, indent + 2);
        }
    }
}

pub fn hexdump(data: &[u8]) {
    let num_full_lines = data.len() / 16;
    for i in 0..num_full_lines {
        printf!("{:#06x}: ", i * 16);
        let line = &data[i * 16..(i + 1) * 16];
        for b in line.iter() {
            printf!("{:02x} ", *b);
        }
        printf!(" | ");
        for b in line.iter() {
            let c = *b as char;
            if c.is_ascii_graphic() {
                printf!("{}", c);
            } else {
                printf!(".");
            }
        }
        println!();
    }

    if data.len() % 16 != 0 {
        printf!("{:#06x}: ", num_full_lines * 16);
        let line = &data[num_full_lines * 16..];
        for b in line.iter() {
            printf!("{:02x} ", *b);
        }
        for _ in 0..(16 - line.len()) {
            printf!("   ");
        }
        printf!(" | ");
        for b in line.iter() {
            let c = *b as char;
            if c.is_ascii_graphic() {
                printf!("{}", c);
            } else {
                printf!(".");
            }
        }
        println!();
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

    #[allow(clippy::empty_loop)]
    loop {}
}
