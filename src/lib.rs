#![no_std]
#![no_main]

use core::num::NonZeroUsize;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use data::{
    calloc_boxed_slice,
    file::{DirectoryEntry, File},
    regs::rflags::{RFlag, RFlags},
};
use drivers::{
    fs::phys::ext2::Ext2Volume,
    pci,
    vfs::{get_vfs, SeekPosition, OPEN_MODE_BINARY, OPEN_MODE_READ, OPEN_MODE_WRITE},
};
use memory::mem::OsMemoryRegion;
use obsiboot::ObsiBootKernelParameters;
use paging::{
    init_paging, physical_to_virtual, PageTable, DIRECT_MAPPING_OFFSET, PAGE_ACCESSED,
    PAGE_PRESENT, PAGE_RW, PAGE_USER,
};
use process::{
    memory::PROC_USER_STACK_TOP,
    proc::{ThreadGPRegisters, ThreadState},
    scheduler::{CreateProcessOptions, SCHEDULER},
};

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
pub mod percpu;
pub mod process;
pub mod vesa;

const PROGRAM_A: &[u8] = &[
    0x48, 0xbf, 0x18, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0xbe, 0x01, 0x00, 0x00, 0x00, 0xb8,
    0x01, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xeb, 0xfc, 0x41,
];

const PROGRAM_B: &[u8] = &[
    0x48, 0xbf, 0x18, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0xbe, 0x01, 0x00, 0x00, 0x00, 0xb8,
    0x01, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xeb, 0xfc, 0x42,
];

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
            if let Some(stats) = File::get_stats("/system/foobar").unwrap() {
                if !stats.is_directory {
                    panic!("/system/foobar is not a directory");
                }
                println!("{:#?}", stats);
            } else {
                File::mkdir("/system/foobar").unwrap();
            }
            if let Some(stats) = File::get_stats("/system/foobar/bar").unwrap() {
                println!("{:#?}", stats);
            } else {
                File::create("/system/foobar/bar", 0).unwrap();
            }
            let mut file = File::open(
                "/system/foobar/bar",
                OPEN_MODE_BINARY | OPEN_MODE_READ | OPEN_MODE_WRITE,
            )
            .unwrap();

            file.seek(SeekPosition::FromEnd(0)).unwrap();
            file.write(b"HELLO WORLD !\n").unwrap();
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

    let mut memory_a = calloc_boxed_slice(4096);
    memory_a[0..PROGRAM_A.len()].copy_from_slice(PROGRAM_A);

    let mut page_table_a = PageTable::alloc_new().unwrap();

    // Copies the kernel's 256..512 pml4 entries
    page_table_a.map_global_higher_half();

    // Process code
    page_table_a.map_4kb(
        0x2_000_000,
        memory_a.as_ptr() as u64 - DIRECT_MAPPING_OFFSET,
        PAGE_RW | PAGE_ACCESSED | PAGE_USER | PAGE_PRESENT,
        false,
    );

    let opts = CreateProcessOptions {
        name: "sh".to_string(),
        cmdline: "/system/bin/sh".to_string(),
        cwd: "/".to_string(),
        page_table: page_table_a,
        main_thread_state: ThreadState {
            gpregs: ThreadGPRegisters {
                rax: 0,
                rbx: 0,
                rcx: 0,
                rdx: 0,
                rsi: 0,
                rdi: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
            },
            rbp: PROC_USER_STACK_TOP,
            rsp: PROC_USER_STACK_TOP,
            rip: 0x2_000_000,
            rflags: RFlags::empty()
                .set(RFlag::InterruptFlag)
                .set(RFlag::IOPL3)
                .get(),
            fs_base: 0,
            gs_base: 0,
        },
    };
    SCHEDULER.create_process(opts);

    let mut memory_b = calloc_boxed_slice(4096);
    memory_b[0..PROGRAM_B.len()].copy_from_slice(PROGRAM_B);

    let mut page_table_b = PageTable::alloc_new().unwrap();

    // Copies the kernel's 256..512 pml4 entries
    page_table_b.map_global_higher_half();

    // Process code
    page_table_b.map_4kb(
        0x2_000_000,
        memory_b.as_ptr() as u64 - DIRECT_MAPPING_OFFSET,
        PAGE_RW | PAGE_ACCESSED | PAGE_USER | PAGE_PRESENT,
        false,
    );

    let opts = CreateProcessOptions {
        name: "ls".to_string(),
        cmdline: "/system/bin/ls".to_string(),
        cwd: "/".to_string(),
        page_table: page_table_b,
        main_thread_state: ThreadState {
            gpregs: ThreadGPRegisters {
                rax: 0,
                rbx: 0,
                rcx: 0,
                rdx: 0,
                rsi: 0,
                rdi: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
            },
            rbp: PROC_USER_STACK_TOP,
            rsp: PROC_USER_STACK_TOP,
            rip: 0x2_000_000,
            rflags: RFlags::empty()
                .set(RFlag::InterruptFlag)
                .set(RFlag::IOPL3)
                .get(),
            fs_base: 0,
            gs_base: 0,
        },
    };
    SCHEDULER.create_process(opts);

    SCHEDULER.schedule();
}
