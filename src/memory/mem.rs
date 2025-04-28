use crate::{
    memory::buddy_alloc::{self, BuddyPageAllocator},
    paging::{align_up, physical_to_virtual, MB2},
    printf, println,
};

#[derive(Default)]
pub struct GlobalAlloc {}

impl GlobalAlloc {
    pub const fn new() -> Self {
        Self {}
    }
}

unsafe impl core::alloc::GlobalAlloc for GlobalAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::null_mut();
        }
        if layout.align() > 4096 {
            return core::ptr::null_mut();
        }
        #[allow(static_mut_refs)]
        match &mut MAIN_BUDDY_ALLOCATOR {
            None => panic!(
                "Try to allocate memory without an allocator !\n{:#?}",
                layout
            ),
            Some(allocator) => allocator
                .alloc(layout.size() as u64)
                .map(|addr| addr as *mut u8)
                .unwrap_or(core::ptr::null_mut()),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if layout.size() == 0 {
            return;
        }
        if layout.align() > 4096 {
            return;
        }
        #[allow(static_mut_refs)]
        match &mut MAIN_BUDDY_ALLOCATOR {
            None => {}
            Some(allocator) => allocator.free(ptr as u64),
        }
    }
}

#[global_allocator]
static GLOBAL_ALLOC: GlobalAlloc = GlobalAlloc::new();

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct OsMemoryRegion {
    pub start: u64,
    pub end: u64,
    pub usable: u64,
}

pub struct ExtendedBuddyPageAllocator {
    allocator: BuddyPageAllocator,
    orders: *mut u8,
}

impl ExtendedBuddyPageAllocator {
    pub fn new(mut allocator: BuddyPageAllocator) -> Option<Self> {
        let (orders, o) =
            allocator.alloc(allocator.get_page_count().div_ceil(buddy_alloc::PAGE_SIZE))?;

        unsafe {
            core::ptr::write_bytes(orders as *mut u8, 0xFF, allocator.get_page_count() as usize);
        }

        let mut v = Self {
            allocator,
            orders: orders as *mut u8,
        };

        v.mark_used(orders, o);

        Some(v)
    }

    #[inline(always)]
    fn mark_used(&mut self, addr: u64, order: u8) {
        let i = (addr - self.allocator.get_base_addr()) / buddy_alloc::PAGE_SIZE;
        unsafe {
            *self.orders.add(i as usize) = order;
        }
    }

    #[inline(always)]
    fn mark_free(&mut self, addr: u64) {
        let i = (addr - self.allocator.get_base_addr()) / buddy_alloc::PAGE_SIZE;
        unsafe {
            *self.orders.add(i as usize) = 0xFF;
        }
    }

    #[inline(always)]
    fn get_order(&mut self, addr: u64) -> Option<u8> {
        let i = (addr - self.allocator.get_base_addr()) / buddy_alloc::PAGE_SIZE;
        match unsafe { *self.orders.add(i as usize) } {
            0xFF => None,
            v => Some(v),
        }
    }

    #[inline(always)]
    fn is_in_range(&self, addr: u64) -> bool {
        let i = (addr - self.allocator.get_base_addr()) / buddy_alloc::PAGE_SIZE;
        i < self.allocator.get_page_count()
    }

    /// Allocates a block of at least `size` bytes, 4 KiB aligned, continuous, not zeroed
    pub fn alloc(&mut self, size: u64) -> Option<u64> {
        let (addr, order) = self.allocator.alloc(size / buddy_alloc::PAGE_SIZE)?;
        self.mark_used(addr, order);
        Some(addr)
    }

    /// Allocates a block of at least `size` bytes, 4 KiB aligned, continuous, zeroed
    pub fn calloc(&mut self, size: u64) -> Option<u64> {
        let addr = self.alloc(size)?;
        unsafe {
            core::ptr::write_bytes(addr as *mut u8, 0, size as usize);
        }
        Some(addr)
    }

    /// Frees an block from its address, `addr` must be 4 KiB aligned
    pub fn free(&mut self, addr: u64) {
        if !self.is_in_range(addr) {
            return;
        }

        let order = match self.get_order(addr) {
            Some(o) => o,
            None => return,
        };

        self.allocator.free(addr, order as u64);
        self.mark_free(addr);
    }
}

static mut MAIN_BUDDY_ALLOCATOR: Option<ExtendedBuddyPageAllocator> = None;

/// # Safety
/// `memory_layout_ptr` must point to a valid memory layout, and `memory_layout_entries` must be a valid number
pub unsafe fn init(
    memory_layout_ptr: *const OsMemoryRegion,
    memory_layout_entries: u64,
    pml4_ptr_phys: u64,
    begin_usable_memory: u64,
) {
    printf!(
        "Memory layout at: {:?} ({} entries)\n=== BEGIN MEMORY LAYOUT DUMP ===\n",
        memory_layout_ptr,
        memory_layout_entries
    );
    for i in 0..memory_layout_entries {
        let region = memory_layout_ptr.offset(i as isize).read_unaligned();
        let (s, e, u) = (region.start, region.end, region.usable);
        printf!(
            "REGION: {:016x} --> {:016x} (usable:{})\n",
            s,
            e,
            match u {
                0 => "no",
                _ => "yes",
            }
        );
    }
    printf!("===  END MEMORY LAYOUT DUMP  ===\n\n");

    for i in 0..memory_layout_entries {
        let region = memory_layout_ptr.offset(i as isize).read_unaligned();
        let (s, e, u) = (region.start, region.end, region.usable);
        if u == 0 || s < 0x10000 || e < 0x10000 {
            continue;
        }

        let s = if s == pml4_ptr_phys {
            align_up(begin_usable_memory, MB2 as u64)
        } else {
            s
        };

        let start = physical_to_virtual(s);
        let end = physical_to_virtual(e);

        println!("Found usable memory region: {:#x} --> {:#x}", start, end);

        #[allow(static_mut_refs)]
        match MAIN_BUDDY_ALLOCATOR {
            None => {
                let alloc = BuddyPageAllocator::new(start, (end - start) / 4096);
                MAIN_BUDDY_ALLOCATOR = Some(
                    ExtendedBuddyPageAllocator::new(alloc)
                        .expect("Failed to initialize main buddy allocator."),
                );
            }
            Some(_) => {
                unimplemented!("Multiple memory regions are not supported yet.");
            }
        }
    }
}
