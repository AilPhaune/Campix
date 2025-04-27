use core::arch::asm;
use core::ptr::addr_of;

use dc_access::{
    ACCESSED, CODE_READ, CODE_SEGMENT, DATA_SEGMENT, DATA_WRITE, PRESENT, RING0, RING1, RING2,
    RING3,
};
use flags::{GRANULARITY_4KB, IS_32BIT, LONG_MODE};

use crate::println;

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

pub struct KernelGdt([GdtEntry; 7]);

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

pub const GDT: KernelGdt = KernelGdt([
    GdtEntry::new(0, 0, 0, 0), // Null descriptor
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
]);

pub const KERNEL_CODE_SELECTOR: usize = GDT
    .get_code_selector(GdtRing::Ring0, GdtBits::Bits64)
    .unwrap();
pub const KERNEL_DATA_SELECTOR: usize = GDT
    .get_data_selector(GdtRing::Ring0, GdtBits::Bits64)
    .unwrap();

#[no_mangle]
pub static mut GDTR: GdtDescriptor = GdtDescriptor { limit: 0, base: 0 };

#[allow(static_mut_refs)]
pub(crate) unsafe fn init_gdtr() {
    GDTR = GdtDescriptor {
        limit: size_of::<[GdtEntry; 7]>() as u16 - 1,
        base: GDT.0.as_ptr() as u64,
    };

    println!("Kernel GDT at {:#016x}", GDT.0.as_ptr() as u64);
    for i in 0..7 {
        println!("  Descriptor #{}: {:016x}", i, GDT.0[i].into());
    }
    println!("GDTR at {:#016x}", addr_of!(GDTR) as u64);

    println!("Kernel code selector: {:#x}", KERNEL_CODE_SELECTOR);
    println!("Kernel data selector: {:#x}", KERNEL_DATA_SELECTOR);
    println!();

    asm!("lgdt [{}]", in(reg) &GDTR, options(readonly, nostack, preserves_flags));
    asm!(
        "push {0}",
        "lea rax, [rip + 2f]",
        "push rax",
        "retfq",
        "2:",
        "mov ds, {1}",
        "mov es, {1}",
        "mov fs, {1}",
        "mov gs, {1}",
        "mov ss, {1}",
        in(reg) KERNEL_CODE_SELECTOR,
        in(reg) KERNEL_DATA_SELECTOR,
        out("rax") _,
        options(nostack)
    );
}
