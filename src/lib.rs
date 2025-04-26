#![no_std]
#![no_main]

use memory::OsMemoryRegion;
use paging::{init_paging, physical_to_virtual, DIRECT_MAPPING_OFFSET};

extern crate alloc;

pub mod buddy_alloc;
pub mod e9;
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
        println!("CampiOS Kernel");
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

        memory::init(
            physical_to_virtual(memory_layout_ptr) as *const OsMemoryRegion,
            memory_layout_entries,
            pml4_ptr_phys,
            begin_usable_memory,
        );

        kmain(
            physical_to_virtual(memory_layout_ptr) as *const OsMemoryRegion,
            memory_layout_entries,
        );
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
    printf!("Panic: {}\r\n", info.message());

    match info.location() {
        Some(loc) => {
            printf!("Location: {}\r\n", loc);
        }
        None => printf!("Location unknown !\r\n"),
    }
}

unsafe fn kmain(memory_layout_ptr: *const OsMemoryRegion, memory_layout_entries: u64) -> ! {
    let message = "Welcome to CampiOS !";
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

    printf!(
        "Memory layout at: {:?} ({} entries)\r\n=== BEGIN MEMORY LAYOUT DUMP ===\r\n",
        memory_layout_ptr,
        memory_layout_entries
    );
    for i in 0..memory_layout_entries {
        let region = memory_layout_ptr.offset(i as isize).read_unaligned();
        let (s, e, u) = (region.start, region.end, region.usable);
        printf!(
            "REGION: {:016x} --> {:016x} (usable:{})\r\n",
            s,
            e,
            match u {
                0 => "no",
                _ => "yes",
            }
        );
    }
    printf!("===  END MEMORY LAYOUT DUMP  ===\r\n\n");

    #[allow(clippy::empty_loop)]
    loop {}
}
