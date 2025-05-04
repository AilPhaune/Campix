#![no_std]
#![no_main]
#![feature(negative_impls)]

use drivers::{pci, vga::use_vga_device_mut};
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
            use_vga_device_mut(|vga| {
                let width = vga.get_width() as usize;
                let height = vga.get_height() as usize;
                let max_iter = 400;

                // Complex plane bounds
                let scale_x = 3.5 / width as f64;
                let scale_y = 2.0 / height as f64;

                for y in 0..height {
                    let cy = y as f64 * scale_y - 1.0;

                    for x in 0..width {
                        let cx = x as f64 * scale_x - 2.5;
                        let mut zx = 0.0;
                        let mut zy = 0.0;
                        let mut iter = 0;

                        while zx * zx + zy * zy <= 4.0 && iter < max_iter {
                            let xtemp = zx * zx - zy * zy + cx;
                            zy = 2.0 * zx * zy + cy;
                            zx = xtemp;
                            iter += 1;
                        }

                        let color = if iter == max_iter {
                            0x000000 // black
                        } else {
                            let intensity = (255 * iter / max_iter) as u8;
                            ((intensity as u32) << 16) | ((intensity as u32) << 8)
                            // red+green = yellow gradient
                        };

                        let offset = y * width + x;
                        vga.write_pixel_at_offset(offset as u64, color);
                    }
                }

                vga.swap_buffers();
            });
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
    vesa::parse_current_mode(&obsiboot);
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
