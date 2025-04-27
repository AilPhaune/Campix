use core::arch::asm;
use core::panic;

use crate::{memory::mem::OsMemoryRegion, println};

#[repr(C, align(4096))]
pub struct FreePage {
    pub next: *mut FreePage,
    pub this_size_in_pages: u64,
}

pub struct SimplePageAllocator {
    pub min_addr: u64,
    pub max_addr: u64,
    pub free_head: *mut FreePage,
}

impl SimplePageAllocator {
    pub fn alloc_page(&mut self) -> Option<*mut u8> {
        let free_page = self.free_head;

        if free_page.is_null() {
            return None;
        }

        unsafe {
            if (*free_page).this_size_in_pages == 1 {
                self.free_head = (*free_page).next;
            } else {
                let next_page = (free_page as *mut u8).add(4096) as *mut FreePage;
                self.free_head = if (next_page as u64) < self.max_addr {
                    *next_page = FreePage {
                        next: (*free_page).next,
                        this_size_in_pages: (*free_page).this_size_in_pages - 1,
                    };
                    next_page
                } else {
                    (*free_page).next
                };
            };

            core::ptr::write_bytes(free_page as *mut u8, 0, 4096);

            Some(free_page as *mut u8)
        }
    }

    pub fn free_page(&mut self, page: *mut u8) {
        if (page as usize) & 0xFFF != 0 {
            println!("Trying to free unaligned page !");
            return;
        }

        unsafe {
            let page = page as *mut FreePage;
            *page = FreePage {
                next: self.free_head,
                this_size_in_pages: 1,
            };
            self.free_head = page;
        }
    }
}

static mut PML4: *mut u64 = core::ptr::null_mut();
static mut ALLOCATOR: SimplePageAllocator = SimplePageAllocator {
    min_addr: 0,
    max_addr: 0,
    free_head: core::ptr::null_mut(),
};

pub const DIRECT_MAPPING_OFFSET: u64 = 0xFFFF_A000_0000_0000;

pub fn physical_to_virtual(phys: u64) -> u64 {
    phys + DIRECT_MAPPING_OFFSET
}

pub fn ptr_of_phys<T>(phys: *mut T) -> *mut T {
    physical_to_virtual(phys as u64) as *mut T
}

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SIZE_2MB: usize = 2 * 1024 * 1024;

// Page Table Entry Flags
pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_RW: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_WRITE_THROUGH: u64 = 1 << 3;
pub const PAGE_CACHE_DISABLE: u64 = 1 << 4;
pub const PAGE_ACCESSED: u64 = 1 << 5;
pub const PAGE_DIRTY: u64 = 1 << 6;
pub const PAGE_HUGE: u64 = 1 << 7;
pub const PAGE_GLOBAL: u64 = 1 << 8;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

pub const KB4: usize = 4 * 1024;
pub const MB2: usize = 2 * 1024 * 1024;

// Helper to extract indices for 4-level paging
fn split_virt_addr(addr: u64) -> (usize, usize, usize, usize) {
    let pml4 = ((addr >> 39) & 0x1FF) as usize;
    let pdpt = ((addr >> 30) & 0x1FF) as usize;
    let pd = ((addr >> 21) & 0x1FF) as usize;
    let pt = ((addr >> 12) & 0x1FF) as usize;
    (pml4, pdpt, pd, pt)
}

// Align address down to nearest 4 KiB or 2 MiB
pub fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

// Align address up to nearest 4 KiB or 2 MiB
pub fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// # Safety
/// `memory_layout_ptr` must point to a valid memory layout
pub unsafe fn init_paging(
    memory_layout_ptr: *const OsMemoryRegion,
    memory_layout_entries: u64,
    pml4_ptr_phys: u64,
    page_alloc_curr: u64,
    page_alloc_end: u64,
) {
    let free_page_count = (page_alloc_end - page_alloc_curr) / 4096;
    if free_page_count == 0 {
        panic!("Not enough memory for paging");
    }
    ALLOCATOR = SimplePageAllocator {
        min_addr: physical_to_virtual(pml4_ptr_phys),
        max_addr: physical_to_virtual(page_alloc_end),
        free_head: physical_to_virtual(page_alloc_curr) as *mut FreePage,
    };
    *ALLOCATOR.free_head = FreePage {
        next: core::ptr::null_mut(),
        this_size_in_pages: free_page_count,
    };
    PML4 = physical_to_virtual(pml4_ptr_phys) as *mut u64;

    // Unmap the identity mapping

    // 256 * 4KiB = 1MiB
    for i in 0..256 {
        let addr = (i * KB4) as u64;
        unmap_page_4kb(addr);
    }

    // Usable regions
    let memory_layout_ptr = ptr_of_phys(memory_layout_ptr as *mut OsMemoryRegion);
    for i in 0..memory_layout_entries {
        let region = memory_layout_ptr.add(i as usize).read_unaligned();
        if region.usable == 0 || region.start < (1024 * 1024) {
            continue;
        }

        let aligned_start = align_up(region.start, MB2 as u64);
        let aligned_end = align_down(region.end, MB2 as u64);

        let mut addr = aligned_start;
        while addr < aligned_end {
            unmap_page_2mb(addr);
            addr += MB2 as u64;
        }

        let kb4_aligned_start = align_up(region.start, KB4 as u64);
        let mut addr = kb4_aligned_start;
        while addr < aligned_start {
            unmap_page_4kb(addr);
            addr += KB4 as u64;
        }

        let kb4_aligned_end = align_down(region.end, KB4 as u64);
        let mut addr = aligned_end;
        while addr < kb4_aligned_end {
            unmap_page_4kb(addr);
            addr += KB4 as u64;
        }
    }
}

/// # Safety
/// `virt` and `phys` must be 4 KiB aligned
#[allow(static_mut_refs)]
pub unsafe fn map_page_4kb(virt: u64, phys: u64, flags: u64) {
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

    let pml4_entry = &mut *PML4.add(pml4_idx);
    let pdpt_ptr = if *pml4_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pml4_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        let new = ALLOCATOR
            .alloc_page()
            .expect("Out of memory for page tables");
        *pml4_entry = (new as u64 - DIRECT_MAPPING_OFFSET) | PAGE_PRESENT | PAGE_RW;
        new as *mut u64
    };

    let pdpt_entry = &mut *pdpt_ptr.add(pdpt_idx);
    let pd_ptr = if *pdpt_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pdpt_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        let new = ALLOCATOR
            .alloc_page()
            .expect("Out of memory for page tables");
        *pdpt_entry = (new as u64 - DIRECT_MAPPING_OFFSET) | PAGE_PRESENT | PAGE_RW;
        new as *mut u64
    };

    let pd_entry = &mut *pd_ptr.add(pd_idx);
    let pt_ptr = if *pd_entry & PAGE_PRESENT != 0 {
        if *pd_entry & PAGE_HUGE != 0 {
            panic!("Cannot map 4 KiB page over existing 2 MiB huge page");
        }
        ptr_of_phys((*pd_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        let new = ALLOCATOR
            .alloc_page()
            .expect("Out of memory for page tables");
        *pd_entry = (new as u64 - DIRECT_MAPPING_OFFSET) | PAGE_PRESENT | PAGE_RW;
        new as *mut u64
    };

    let pt_entry = &mut *pt_ptr.add(pt_idx);
    *pt_entry = align_down(phys, PAGE_SIZE as u64) | flags | PAGE_PRESENT;

    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

/// # Safety
/// `virt` and `phys` must be 2 MiB aligned
#[allow(static_mut_refs)]
pub unsafe fn map_page_2mb(virt: u64, phys: u64, flags: u64) {
    let (pml4_idx, pdpt_idx, pd_idx, _) = split_virt_addr(virt);

    let pml4_entry = &mut *PML4.add(pml4_idx);
    let pdpt_ptr = if *pml4_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pml4_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        let new = ALLOCATOR
            .alloc_page()
            .expect("Out of memory for page tables");
        *pml4_entry = (new as u64 - DIRECT_MAPPING_OFFSET) | PAGE_PRESENT | PAGE_RW;
        new as *mut u64
    };

    let pdpt_entry = &mut *pdpt_ptr.add(pdpt_idx);
    let pd_ptr = if *pdpt_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pdpt_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        let new = ALLOCATOR
            .alloc_page()
            .expect("Out of memory for page tables");
        *pdpt_entry = (new as u64 - DIRECT_MAPPING_OFFSET) | PAGE_PRESENT | PAGE_RW;
        new as *mut u64
    };

    let pd_entry = &mut *pd_ptr.add(pd_idx);
    *pd_entry = align_down(phys, PAGE_SIZE_2MB as u64) | flags | PAGE_PRESENT | PAGE_HUGE;

    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

unsafe fn free_if_empty(page: *mut u64) -> bool {
    if core::slice::from_raw_parts(page, 512)
        .iter()
        .all(|x| *x == 0)
    {
        #[allow(static_mut_refs)]
        ALLOCATOR.free_page(page as *mut u8);
        true
    } else {
        false
    }
}

/// # Safety
/// `virt` must be 4 KiB aligned
pub unsafe fn unmap_page_4kb(virt: u64) {
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

    let pml4_entry = &mut *PML4.add(pml4_idx);
    let pdpt_ptr = if *pml4_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pml4_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        return;
    };

    let pdpt_entry = &mut *pdpt_ptr.add(pdpt_idx);
    let pd_ptr = if *pdpt_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pdpt_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        return;
    };

    let pd_entry = &mut *pd_ptr.add(pd_idx);
    let pt_ptr = if *pd_entry & PAGE_PRESENT != 0 && *pd_entry & PAGE_HUGE == 0 {
        ptr_of_phys((*pd_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        return;
    };

    let pt_entry = &mut *pt_ptr.add(pt_idx);
    *pt_entry = 0;

    // Free page table if empty
    if free_if_empty(pt_ptr) {
        *pd_entry = 0;
        if free_if_empty(pd_ptr) {
            *pdpt_entry = 0;
            if free_if_empty(pdpt_ptr) {
                *pml4_entry = 0;
            }
        }
    }

    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

/// # Safety
/// `virt` must be 2 MiB aligned
pub unsafe fn unmap_page_2mb(virt: u64) {
    let (pml4_idx, pdpt_idx, pd_idx, _) = split_virt_addr(virt);

    let pml4_entry = &mut *PML4.add(pml4_idx);
    let pdpt_ptr = if *pml4_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pml4_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        return;
    };

    let pdpt_entry = &mut *pdpt_ptr.add(pdpt_idx);
    let pd_ptr = if *pdpt_entry & PAGE_PRESENT != 0 {
        ptr_of_phys((*pdpt_entry & 0x000F_FFFF_FFFF_F000) as *mut u64)
    } else {
        return;
    };

    let pd_entry = &mut *pd_ptr.add(pd_idx);
    *pd_entry = 0;

    // Free page table if empty
    if free_if_empty(pd_ptr) {
        *pdpt_entry = 0;
        if free_if_empty(pdpt_ptr) {
            *pml4_entry = 0;
        }
    }

    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

pub fn virtual_to_physical(virt: u64) -> u64 {
    unsafe {
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

        let pml4_entry = &*PML4.add(pml4_idx);
        if *pml4_entry & PAGE_PRESENT == 0 {
            return 0;
        }
        let pdpt_ptr = (*pml4_entry & 0x000F_FFFF_FFFF_F000) as *mut u64;

        let pdpt_entry = &*ptr_of_phys(pdpt_ptr.add(pdpt_idx));
        if *pdpt_entry & PAGE_PRESENT == 0 {
            return 0;
        }
        let pd_ptr = (*pdpt_entry & 0x000F_FFFF_FFFF_F000) as *mut u64;

        let pd_entry = &*ptr_of_phys(pd_ptr.add(pd_idx));
        if *pd_entry & PAGE_PRESENT == 0 {
            return 0;
        }

        if *pd_entry & PAGE_HUGE != 0 {
            // 2 MiB huge page
            let base = *pd_entry & 0x000F_FFFF_FFFF_F000;
            let offset = virt & 0x1F_FFFF; // 2 MiB - 1
            return base + offset;
        }

        let pt_ptr = (*pd_entry & 0x000F_FFFF_FFFF_F000) as *mut u64;
        let pt_entry = &*ptr_of_phys(pt_ptr.add(pt_idx));
        if *pt_entry & PAGE_PRESENT == 0 {
            return 0;
        }

        let base = *pt_entry & 0x000F_FFFF_FFFF_F000;
        let offset = virt & 0xFFF; // 4 KiB - 1
        base + offset
    }
}
