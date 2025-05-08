#![no_std]
#![no_main]

use alloc::string::String;
use data::file::File;
use drivers::{
    pci,
    vfs::{OPEN_MODE_BINARY, OPEN_MODE_READ},
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

        gdt::init_gdtr();
        interrupts::init();

        memory::mem::init(
            physical_to_virtual(obsiboot.ptr_to_memory_layout as u64) as *const OsMemoryRegion,
            obsiboot.memory_layout_entry_count as u64,
            obsiboot.pml4_base_address as u64,
            obsiboot.usable_kernel_memory_start as u64,
        );
        vesa::parse_current_mode(&obsiboot);

        {
            println!("\nEnumerating PCI devices:");
            let devices = pci::scan_bus();
            for device in devices.iter() {
                println!("{:?}", device);
            }
        }

        {
            println!("\nListing /dev directory:");
            for entry in File::list_directory("/dev").unwrap().iter() {
                println!("{}", entry.full_name().iter().collect::<String>());
            }
            println!();

            let file = File::open("/dev/pata_pm_p0", OPEN_MODE_READ | OPEN_MODE_BINARY).unwrap();

            let mut buf = [0u8; 2048];
            let bytes_read = file.read(&mut buf).unwrap();
            println!("Read {} bytes from /dev/pata_pm_p0 :", bytes_read);

            // hexdump
            hexdump(&buf[0..bytes_read as usize]);
            println!();
        }

        kmain(obsiboot);
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
