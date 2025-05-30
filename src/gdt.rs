use core::arch::asm;
use core::ptr::addr_of;

use dc_access::{
    ACCESSED, CODE_READ, CODE_SEGMENT, DATA_SEGMENT, DATA_WRITE, PRESENT, RING0, RING1, RING2,
    RING3,
};
use flags::{GRANULARITY_4KB, IS_32BIT, LONG_MODE};

use crate::{
    println,
    process::task::{get_tss_addr, RawTaskStateSegment},
};

pub mod dc_access {
    pub const PRESENT: u8 = 1 << 7;
    pub const RING0: u8 = 0 << 5;
    pub const RING1: u8 = 1 << 5;
    pub const RING2: u8 = 2 << 5;
    pub const RING3: u8 = 3 << 5;
    pub const CODE_SEGMENT: u8 = 0b0001_1000;
    pub const DATA_SEGMENT: u8 = 0b0001_0000;
    pub const DATA_DIRECTION_DOWN: u8 = 1 << 2;
    pub const CODE_DPL: u8 = 1 << 2;
    pub const CODE_READ: u8 = 1 << 1;
    pub const DATA_WRITE: u8 = 1 << 1;
    pub const ACCESSED: u8 = 1 << 0;
}

pub mod flags {
    pub const GRANULARITY_4KB: u8 = 0b1000;
    pub const IS_32BIT: u8 = 0b0100;
    pub const LONG_MODE: u8 = 0b0010;
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtDescriptor {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    flags_limit_high: u8,
    base_high: u8,
}

impl GdtEntry {
    const fn new(base: u32, limit: u32, access: u8, flags: u8) -> GdtEntry {
        GdtEntry {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_mid: ((base >> 16) & 0xFF) as u8,
            access,
            flags_limit_high: (((limit >> 16) & 0x0F) as u8) | (flags << 4),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }

    const fn into(self) -> u64 {
        self.limit_low as u64
            | (self.base_low as u64) << 16
            | (self.base_mid as u64) << 24
            | (self.access as u64) << 40
            | (self.flags_limit_high as u64) << 48
            | (self.base_high as u64) << 56
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TssEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    flags_limit_high: u8,
    base_high: u8,
    base_upper: u32,
    reserved: u32,
}

impl TssEntry {
    pub const fn new(base: u64, limit: u32) -> TssEntry {
        TssEntry {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_mid: ((base >> 16) & 0xFF) as u8,
            access: 0x89, // Present + Available 64-bit TSS
            flags_limit_high: (((limit >> 16) & 0x0F) as u8),
            base_high: ((base >> 24) & 0xFF) as u8,
            base_upper: (base >> 32) as u32,
            reserved: 0,
        }
    }
}

pub enum GdtBits {
    Bits16,
    Bits32,
    Bits64,
}

impl GdtBits {
    const fn as_flags(&self) -> u8 {
        match self {
            GdtBits::Bits16 => 0,
            GdtBits::Bits32 => IS_32BIT,
            GdtBits::Bits64 => LONG_MODE,
        }
    }
}

pub enum GdtRing {
    Ring0,
    Ring1,
    Ring2,
    Ring3,
}

impl GdtRing {
    const fn as_access_flag(&self) -> u8 {
        match self {
            GdtRing::Ring0 => RING0,
            GdtRing::Ring1 => RING1,
            GdtRing::Ring2 => RING2,
            GdtRing::Ring3 => RING3,
        }
    }
}

#[repr(C, packed)]
pub struct KernelGdt([GdtEntry; 11], TssEntry);

impl KernelGdt {
    pub const fn get_selector(
        &self,
        ring: GdtRing,
        bits: GdtBits,
        code_seg: bool,
    ) -> Option<usize> {
        let mut i = 0;
        loop {
            if i >= self.0.len() {
                return None;
            }
            let entry = self.0[i];
            let flags = entry.flags_limit_high >> 4;
            if entry.access & (PRESENT | ring.as_access_flag()) == (PRESENT | ring.as_access_flag())
                && flags & bits.as_flags() == bits.as_flags()
                && entry.access & (DATA_SEGMENT | CODE_SEGMENT)
                    == if code_seg { CODE_SEGMENT } else { DATA_SEGMENT }
            {
                return Some(i * 8);
            }
            i += 1;
        }
    }

    pub const fn get_code_selector(&self, ring: GdtRing, bits: GdtBits) -> Option<usize> {
        self.get_selector(ring, bits, true)
    }

    pub const fn get_data_selector(&self, ring: GdtRing, bits: GdtBits) -> Option<usize> {
        self.get_selector(ring, bits, false)
    }
}

pub static GDT: KernelGdt = KernelGdt(
    [
        GdtEntry::new(0, 0, 0, 0), // Null descriptor
        /*
         * KERNEL
         */
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | CODE_SEGMENT | CODE_READ | ACCESSED,
            GRANULARITY_4KB | IS_32BIT,
        ), // 32-bit Code
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | DATA_SEGMENT | DATA_WRITE | ACCESSED,
            GRANULARITY_4KB | IS_32BIT,
        ), // 32-bit Data
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | CODE_SEGMENT | CODE_READ | ACCESSED,
            0,
        ), // 16-bit Code
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | DATA_SEGMENT | DATA_WRITE | ACCESSED,
            0,
        ), // 16-bit Data
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | CODE_SEGMENT | CODE_READ | ACCESSED,
            GRANULARITY_4KB | LONG_MODE,
        ), // 64-bit Code
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING0 | DATA_SEGMENT | DATA_WRITE | ACCESSED,
            GRANULARITY_4KB | LONG_MODE,
        ), // 64-bit Data
        /*
         * USERLAND
         */
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING3 | CODE_SEGMENT | CODE_READ | ACCESSED,
            GRANULARITY_4KB | LONG_MODE,
        ), // 64-bit Code
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING3 | DATA_SEGMENT | DATA_WRITE | ACCESSED,
            GRANULARITY_4KB | LONG_MODE,
        ), // 64-bit Data
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING3 | CODE_SEGMENT | CODE_READ | ACCESSED,
            GRANULARITY_4KB | IS_32BIT,
        ), // 32-bit Code
        GdtEntry::new(
            0,
            u32::MAX,
            PRESENT | RING3 | DATA_SEGMENT | DATA_WRITE | ACCESSED,
            GRANULARITY_4KB | IS_32BIT,
        ), // 32-bit Data
    ],
    TssEntry::new(0, 0),
);

pub const KERNEL_CODE_SELECTOR: usize = GDT
    .get_code_selector(GdtRing::Ring0, GdtBits::Bits64)
    .unwrap();
pub const KERNEL_DATA_SELECTOR: usize = GDT
    .get_data_selector(GdtRing::Ring0, GdtBits::Bits64)
    .unwrap();

pub const USERLAND_DATA32_SELECTOR: usize = GDT
    .get_data_selector(GdtRing::Ring3, GdtBits::Bits32)
    .unwrap();
pub const USERLAND_CODE32_SELECTOR: usize = GDT
    .get_code_selector(GdtRing::Ring3, GdtBits::Bits32)
    .unwrap();

pub const USERLAND_DATA64_SELECTOR: usize = GDT
    .get_data_selector(GdtRing::Ring3, GdtBits::Bits64)
    .unwrap();
pub const USERLAND_CODE64_SELECTOR: usize = GDT
    .get_code_selector(GdtRing::Ring3, GdtBits::Bits64)
    .unwrap();

pub const TSS_SELECTOR: usize = GDT.0.len() * size_of::<GdtEntry>();

#[no_mangle]
pub static mut GDTR: GdtDescriptor = GdtDescriptor { limit: 0, base: 0 };

#[allow(static_mut_refs)]
pub(crate) unsafe fn init_gdtr() {
    GDTR = GdtDescriptor {
        limit: size_of::<KernelGdt>() as u16 - 1,
        base: addr_of!(GDT) as u64,
    };

    unsafe {
        (&GDT.1 as *const TssEntry as *mut TssEntry).write(TssEntry::new(
            get_tss_addr(),
            size_of::<RawTaskStateSegment>() as u32 - 1,
        ));
    }

    println!("Kernel GDT at {:#016x}", GDT.0.as_ptr() as u64);
    for i in 0..GDT.0.len() {
        println!("  Descriptor #{}: {:016x}", i, GDT.0[i].into());
    }
    println!("GDTR at {:#016x}", addr_of!(GDTR) as u64);

    println!("Kernel code selector: {:#x}", KERNEL_CODE_SELECTOR);
    println!("Kernel data selector: {:#x}", KERNEL_DATA_SELECTOR);
    println!("Userland code32 selector: {:#x}", USERLAND_CODE32_SELECTOR);
    println!("Userland data32 selector: {:#x}", USERLAND_DATA32_SELECTOR);
    println!("Userland code64 selector: {:#x}", USERLAND_CODE64_SELECTOR);
    println!("Userland data64 selector: {:#x}", USERLAND_DATA64_SELECTOR);
    println!("TSS selector: {:#x}", TSS_SELECTOR);

    println!("Kernel TSS at {:#016x}", get_tss_addr());
    println!("TSS entry: {:#032x}", unsafe {
        *(&GDT.1 as *const TssEntry as *const u128)
    });

    println!();

    asm!("lgdt [{}]", in(reg) &GDTR, options(readonly, nostack, preserves_flags));
    asm!(
        "ltr {0:x}",
        in(reg) TSS_SELECTOR as u16,
        options(nostack, preserves_flags),
    );
    asm!(
        "push {0}",
        "lea rax, [rip + 2f]",
        "push rax",
        "retfq",
        "2:",
        "mov ds, {1}",
        "mov es, {1}",
        "mov fs, {2:e}",
        "mov gs, {2:e}",
        "mov ss, {1}",
        in(reg) KERNEL_CODE_SELECTOR,
        in(reg) KERNEL_DATA_SELECTOR,
        in(reg) 0,
        out("rax") _,
        options(nostack)
    );
}
