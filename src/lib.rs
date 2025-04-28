#![no_std]
#![no_main]
#![feature(naked_functions)]

use drivers::{
    disk::pata::{PataBus, PataController, PataDrive},
    pci,
};
use memory::mem::OsMemoryRegion;
use paging::{init_paging, physical_to_virtual, DIRECT_MAPPING_OFFSET};

extern crate alloc;

pub mod drivers;
pub mod e9;
pub mod gdt;
pub mod interrupts;
pub mod io;
pub mod memory;
pub mod paging;

#[no_mangle]
pub fn _start(
    memory_layout_ptr: u64,
    memory_layout_entries: u64,
    pml4_ptr_phys: u64,
    page_alloc_curr: u64,
    page_alloc_end: u64,
    begin_usable_memory: u64,
) -> ! {
    unsafe {
        println!("Campix Kernel");
        println!("Memory layout pointer: {:#x}", memory_layout_ptr);
        println!("Memory layout entries: {}", memory_layout_entries);
        println!("PML4 pointer: {:#x}", pml4_ptr_phys);
        println!("Page allocator start: {:#x}", page_alloc_curr);
        println!("Page allocator end: {:#x}", page_alloc_end);
        println!("Begin usable memory: {:#x}", begin_usable_memory);
        println!();

        init_paging(
            memory_layout_ptr as *const OsMemoryRegion,
            memory_layout_entries,
            pml4_ptr_phys,
            page_alloc_curr,
            page_alloc_end,
        );

        gdt::init_gdtr();
        interrupts::init();

        memory::mem::init(
            physical_to_virtual(memory_layout_ptr) as *const OsMemoryRegion,
            memory_layout_entries,
            pml4_ptr_phys,
            begin_usable_memory,
        );

        {
            println!("\nEnumerating PCI devices:");
            let devices = pci::scan_bus();
            for device in devices.iter() {
                println!("{:?}", device);
            }
        }

        kmain();
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

unsafe fn kmain() -> ! {
    let message = "Welcome to Campix !";
    let video_buffer = (0xb8000 + DIRECT_MAPPING_OFFSET) as *mut u8;

    for i in 0..(80 * 25) {
        video_buffer.offset(i * 2 + 1).write_volatile(0x0f);
        video_buffer
            .offset(i * 2)
            .write_volatile(match message.chars().nth(i as usize) {
                None => 0,
                Some(c) => c as u8,
            });
    }

    io::outb(0x3d4, 0x0f);
    io::outb(0x3d5, 80);
    io::outb(0x3d4, 0x0e);
    io::outb(0x3d5, 0);

    #[allow(clippy::empty_loop)]
    loop {}
}
