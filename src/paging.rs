use core::panic;
use core::{alloc::Layout, arch::asm};

use alloc::alloc::{alloc, dealloc};
use spin::mutex::Mutex;

use crate::data::assign_once::AssignOnce;
use crate::data::regs::cr::Cr3;
use crate::{memory::mem::OsMemoryRegion, println};

#[repr(C, align(4096))]
pub struct FreePage {
    pub next: *mut FreePage,
    pub this_size_in_pages: u64,
}

pub trait PageAllocator {
    /// Allocates a new page and returns its virtual address in direct mapping. Does not clear the page
    fn alloc_page(&mut self) -> Option<*mut u8>;
    fn free_page(&mut self, page: *mut u8);
}

pub struct SimplePageAllocator {
    pub min_addr: u64,
    pub max_addr: u64,
    pub free_head: *mut FreePage,
}

impl PageAllocator for SimplePageAllocator {
    fn alloc_page(&mut self) -> Option<*mut u8> {
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

            Some(free_page as *mut u8)
        }
    }

    fn free_page(&mut self, page: *mut u8) {
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

//static mut PML4: *mut u64 = core::ptr::null_mut();
static mut ALLOCATOR: SimplePageAllocator = SimplePageAllocator {
    min_addr: 0,
    max_addr: 0,
    free_head: core::ptr::null_mut(),
};

#[allow(static_mut_refs)]
static mut KERNEL_PAGE_TABLE: Mutex<PageTable> = Mutex::new(PageTable::new_with_alloc(0, unsafe {
    (&mut ALLOCATOR) as *mut dyn PageAllocator
}));

static KERNEL_STACK_POINTER: AssignOnce<u64> = AssignOnce::new();

#[allow(static_mut_refs)]
pub fn get_kernel_page_table() -> &'static Mutex<PageTable> {
    unsafe { &KERNEL_PAGE_TABLE }
}

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

// Helper to construct back an address from indices for 4-level paging
fn join_virt_addr(pml4: usize, pdpt: usize, pd: usize, pt: usize, offset: usize) -> u64 {
    let mut addr: u64 = 0;
    addr |= (pml4 as u64) << 39;
    addr |= (pdpt as u64) << 30;
    addr |= (pd as u64) << 21;
    addr |= (pt as u64) << 12;
    addr |= offset as u64 & 0xFFF;

    // Canonical sign extension for bit 47
    if addr & (1 << 47) != 0 {
        addr |= 0xFFFF_0000_0000_0000;
    }

    addr
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
    kernel_stack_pointer: u64,
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

    #[allow(static_mut_refs)]
    let mut alloc = PageTable::new_with_alloc(pml4_ptr_phys, &mut ALLOCATOR);

    // Unmap the identity mapping

    // 256 * 4KiB = 1MiB
    for i in 0..256 {
        let addr = (i * KB4) as u64;
        alloc.unmap_4kb(addr, false);
    }

    // Usable regions
    let memory_layout_ptr = ptr_of_phys(memory_layout_ptr as *mut OsMemoryRegion);
    for i in 0..memory_layout_entries {
        let region = core::ptr::read_volatile(memory_layout_ptr.add(i as usize));
        if region.usable == 0 || region.start < (1024 * 1024) {
            continue;
        }

        let aligned_start = align_up(region.start, MB2 as u64);
        let aligned_end = align_down(region.end, MB2 as u64);

        let mut addr = aligned_start;
        while addr < aligned_end {
            alloc.unmap_2mb(addr, false);
            addr += MB2 as u64;
        }

        let kb4_aligned_start = align_up(region.start, KB4 as u64);
        let mut addr = kb4_aligned_start;
        while addr < aligned_start {
            alloc.unmap_4kb(addr, false);
            addr += KB4 as u64;
        }

        let kb4_aligned_end = align_down(region.end, KB4 as u64);
        let mut addr = aligned_end;
        while addr < kb4_aligned_end {
            alloc.unmap_4kb(addr, false);
            addr += KB4 as u64;
        }
    }

    alloc.load();

    KERNEL_STACK_POINTER.set(kernel_stack_pointer);
    KERNEL_PAGE_TABLE = Mutex::new(alloc);
}

#[repr(transparent)]
struct Table([u64; 512]);

impl Table {
    #[inline(always)]
    fn get_entry(&mut self, idx: usize) -> &mut u64 {
        &mut self.0[idx]
    }

    #[inline(always)]
    fn get_table<const ADD_IF_EMPTY: bool>(
        &mut self,
        idx: usize,
        allocator: &mut dyn PageAllocator,
        sub_flags: u64,
        disallowed_flags: u64,
    ) -> Option<&mut Table> {
        let value = self.0[idx];
        if value & PAGE_PRESENT == 0 {
            if ADD_IF_EMPTY {
                let ptr = allocator.alloc_page()? as *mut Table;
                unsafe {
                    core::ptr::write_bytes(ptr, 0, 1);
                }
                self.0[idx] = ((ptr as u64) - DIRECT_MAPPING_OFFSET) | sub_flags;
            } else {
                return None;
            }
        }
        let value = self.0[idx];

        if value & disallowed_flags != 0 {
            return None;
        }
        let ptr = (value & 0x000F_FFFF_FFFF_F000) + DIRECT_MAPPING_OFFSET;

        Some(unsafe { &mut *(ptr as *mut Table) })
    }

    #[inline(always)]
    fn remove(&mut self, idx: usize, allocator: &mut dyn PageAllocator) -> Option<()> {
        let value = self.0[idx];
        if value & PAGE_PRESENT != 0 {
            let ptr = value & 0x000F_FFFF_FFFF_F000;
            allocator.free_page((ptr + DIRECT_MAPPING_OFFSET) as *mut u8);
        }
        self.0[idx] = 0;
        Some(())
    }

    #[inline(always)]
    fn empty(&self) -> bool {
        self.0.iter().all(|x| *x == 0)
    }
}

pub enum PageSize {
    Kb4,
    Mb2,
    Gb1,
}

pub struct PageTableEntry {
    pub virt: u64,
    pub phys: u64,
    pub page_size: PageSize,
}

pub struct PageTableIter<'a> {
    table: &'a mut PageTable,
    position: u64,
    end_exclusive: u64,
}

impl<'a> Iterator for PageTableIter<'a> {
    type Item = PageTableEntry;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while self.position < self.end_exclusive {
                if self.position >= 0x0000_8000_0000_0000 && self.position < 0xFFFF_8000_0000_0000 {
                    // Invalid address space
                    self.position = 0xFFFF_8000_0000_0000;
                    continue;
                }
                let virt = self.position;
                let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

                let allocator = &mut *self.table.allocator;

                let pml4: &mut Table =
                    &mut *((self.table.get_pml4() + DIRECT_MAPPING_OFFSET) as *mut Table);
                let pdpt = match pml4.get_table::<false>(pml4_idx, allocator, 0, 0) {
                    Some(pdpt) => pdpt,
                    None => {
                        // no pdpt
                        let next_pml4_idx = pml4_idx + 1;
                        if next_pml4_idx >= 512 {
                            break;
                        }
                        self.position = join_virt_addr(next_pml4_idx, 0, 0, 0, 0);
                        continue;
                    }
                };
                let pd = match pdpt.get_table::<false>(pdpt_idx, allocator, 0, 0) {
                    Some(pd) => pd,
                    None => {
                        let mut next_pdpt_idx = pdpt_idx + 1;
                        let next_pml4_idx = if pdpt_idx >= 512 {
                            next_pdpt_idx = 0;
                            pml4_idx + 1
                        } else {
                            pml4_idx
                        };
                        self.position = join_virt_addr(next_pml4_idx, next_pdpt_idx, 0, 0, 0);
                        continue;
                    }
                };

                let pd_entry = *pd.get_entry(pd_idx);
                if (pd_entry & PAGE_PRESENT) == PAGE_PRESENT && (pd_entry & PAGE_HUGE) == PAGE_HUGE
                {
                    self.position += PAGE_SIZE_2MB as u64;

                    let phys = pd_entry & 0x000F_FFFF_FFFF_F000;

                    return Some(PageTableEntry {
                        virt,
                        phys,
                        page_size: PageSize::Mb2,
                    });
                }

                let pt = match pd.get_table::<false>(pd_idx, allocator, 0, PAGE_HUGE) {
                    Some(pt) => pt,
                    None => {
                        let mut next_pd_idx = pd_idx + 1;
                        let mut next_pdpt_idx = if next_pd_idx >= 512 {
                            next_pd_idx = 0;
                            pdpt_idx + 1
                        } else {
                            pdpt_idx
                        };
                        let next_pml4_idx = if next_pdpt_idx >= 512 {
                            next_pdpt_idx = 0;
                            pml4_idx + 1
                        } else {
                            pml4_idx
                        };
                        self.position =
                            join_virt_addr(next_pml4_idx, next_pdpt_idx, next_pd_idx, 0, 0);
                        continue;
                    }
                };
                let pt_entry = *pt.get_entry(pt_idx);
                if (pt_entry & PAGE_PRESENT) == PAGE_PRESENT {
                    self.position += PAGE_SIZE as u64;

                    let phys = pt_entry & 0x000F_FFFF_FFFF_F000;

                    return Some(PageTableEntry {
                        virt,
                        phys,
                        page_size: PageSize::Kb4,
                    });
                }

                self.position += PAGE_SIZE as u64;
            }
            None
        }
    }
}

#[derive(Debug)]
pub struct PageTable {
    allocator: *mut dyn PageAllocator,
    owns_allocator: bool,
    pml4_phys: u64,
    allocator_owns_pml4: bool,
}

unsafe impl Send for PageTable {}

impl PageTable {
    pub const fn new_with_alloc(pml4_phys: u64, allocator: *mut dyn PageAllocator) -> Self {
        Self {
            allocator,
            owns_allocator: false,
            pml4_phys,
            allocator_owns_pml4: false,
        }
    }

    pub fn alloc_new() -> Option<Self> {
        unsafe {
            let allocator =
                alloc(Layout::new::<KernelPageTablesAllocator>()) as *mut KernelPageTablesAllocator;
            *allocator = KernelPageTablesAllocator;

            let Some(pml4) = (*allocator).alloc_page() else {
                dealloc(
                    allocator as u64 as *mut u8,
                    Layout::new::<KernelPageTablesAllocator>(),
                );
                return None;
            };

            core::ptr::write_bytes(pml4, 0, 1);

            Some(Self {
                allocator: allocator as *mut dyn PageAllocator,
                owns_allocator: true,
                pml4_phys: pml4 as u64 - DIRECT_MAPPING_OFFSET,
                allocator_owns_pml4: true,
            })
        }
    }

    pub fn get_pml4(&self) -> u64 {
        self.pml4_phys
    }

    /// # Safety
    /// - `begin_inclusive` and `end_exclusive` must be page aligned
    pub unsafe fn iter_range<'a>(
        &'a mut self,
        begin_inclusive: u64,
        end_exclusive: u64,
    ) -> PageTableIter<'a> {
        PageTableIter {
            table: self,
            position: begin_inclusive,
            end_exclusive,
        }
    }

    /// # Safety
    /// - `virt` must be page aligned <br>
    /// - `phys` must be page aligned and valid <br>
    /// - `flags` must be valid <br>
    pub unsafe fn map_4kb(
        &mut self,
        virt: u64,
        phys: u64,
        flags: u64,
        invalidate: bool,
    ) -> Option<()> {
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

        let sub_flags = if virt >= 0xFFFF_8000_0000_0000 {
            PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED
        } else {
            PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED | PAGE_USER
        };

        let allocator = &mut *self.allocator;

        let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
        let pdpt = pml4.get_table::<true>(pml4_idx, allocator, sub_flags, 0)?;
        let pd = pdpt.get_table::<true>(pdpt_idx, allocator, sub_flags, 0)?;
        let pt = pd.get_table::<true>(pd_idx, allocator, sub_flags, PAGE_HUGE)?;
        *pt.get_entry(pt_idx) = align_down(phys, PAGE_SIZE as u64) | flags;

        if invalidate {
            asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
        }

        Some(())
    }

    /// # Safety
    /// - `virt` must be 2mb aligned <br>
    /// - `phys` must be 2mb aligned and valid <br>
    /// - `flags` must be valid <br>
    pub unsafe fn map_2mb(
        &mut self,
        virt: u64,
        phys: u64,
        flags: u64,
        invalidate: bool,
    ) -> Option<()> {
        let (pml4_idx, pdpt_idx, pd_idx, _) = split_virt_addr(virt);

        let sub_flags = if virt >= 0xFFFF_8000_0000_0000 {
            PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED
        } else {
            PAGE_PRESENT | PAGE_RW | PAGE_ACCESSED | PAGE_USER
        };

        let allocator = &mut *self.allocator;

        let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
        let pdpt = pml4.get_table::<true>(pml4_idx, allocator, sub_flags, 0)?;
        let pd = pdpt.get_table::<true>(pdpt_idx, allocator, sub_flags, 0)?;
        *pd.get_entry(pd_idx) = align_down(phys, PAGE_SIZE_2MB as u64) | PAGE_HUGE | flags;

        if invalidate {
            asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
        }

        Some(())
    }

    /// # Safety
    /// - `virt` must be page aligned <br>
    /// - `flags` must be valid <br>
    pub unsafe fn unmap_4kb(&mut self, virt: u64, invalidate: bool) -> Option<()> {
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);

        let allocator = &mut *self.allocator;

        let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
        let pdpt = pml4.get_table::<false>(pml4_idx, allocator, 0, 0)?;
        let pd = pdpt.get_table::<false>(pdpt_idx, allocator, 0, 0)?;
        let pt = pd.get_table::<false>(pd_idx, allocator, 0, PAGE_HUGE)?;
        *pt.get_entry(pt_idx) = 0;

        if pt.empty() {
            pd.remove(pd_idx, allocator)?;
            if pd.empty() {
                pdpt.remove(pdpt_idx, allocator)?;
                if pdpt.empty() {
                    pml4.remove(pml4_idx, allocator)?;
                }
            }
        }

        if invalidate {
            asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
        }

        Some(())
    }

    /// # Safety
    /// - `virt` must be 2mb aligned <br>
    /// - `flags` must be valid <br>
    pub unsafe fn unmap_2mb(&mut self, virt: u64, invalidate: bool) -> Option<()> {
        let (pml4_idx, pdpt_idx, pd_idx, _) = split_virt_addr(virt);

        let allocator = &mut *self.allocator;

        let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
        let pdpt = pml4.get_table::<false>(pml4_idx, allocator, 0, 0)?;
        let pd = pdpt.get_table::<false>(pdpt_idx, allocator, 0, 0)?;
        *pd.get_entry(pd_idx) = 0;

        if pd.empty() {
            pdpt.remove(pdpt_idx, allocator)?;
            if pdpt.empty() {
                pml4.remove(pml4_idx, allocator)?;
            }
        }

        if invalidate {
            asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
        }

        Some(())
    }

    /// Maps a range of virtual addresses to a range of physical addresses
    /// Translation used is virt = phys + `virt_offset`
    /// Range starts at `addr` and ends at `addr + len`, aligned to 2mb and 4kb boundaries that contain the entire range
    pub fn map_memory(
        &mut self,
        addr: u64,
        len: u64,
        virt_offset: u64,
        flags: u64,
        invalidate: bool,
    ) {
        unsafe {
            let begin_2mb = align_up(addr, MB2 as u64);
            let end_2mb = align_down(addr + len, MB2 as u64);
            let begin_4kb = align_down(addr, KB4 as u64);
            let end_4kb = align_up(addr + len, KB4 as u64);

            let count_maps = ((end_2mb - begin_2mb) / MB2 as u64)
                + ((begin_2mb - begin_4kb) / KB4 as u64)
                + ((end_4kb - end_2mb) / KB4 as u64);

            let invalidate_each = invalidate && count_maps > 32;

            let mut addr = begin_2mb;
            while addr < end_2mb {
                self.map_2mb(addr + virt_offset, addr, flags, invalidate_each);
                addr += MB2 as u64;
            }

            let mut addr = begin_4kb;
            while addr < begin_2mb {
                self.map_4kb(
                    addr + virt_offset,
                    addr,
                    PAGE_ACCESSED | PAGE_RW,
                    invalidate_each,
                );
                addr += KB4 as u64;
            }

            let mut addr = end_2mb;
            while addr < end_4kb {
                self.map_4kb(addr + virt_offset, addr, flags, invalidate_each);
                addr += KB4 as u64;
            }

            if invalidate && !invalidate_each {
                self.invalidate();
            }
        }
    }

    /// # Safety
    /// This function is unsafe because it might modify the CR3 register <br>
    /// Caller must make sure code is running in ring 0 and that the return address is mapped <br>
    pub unsafe fn invalidate(&mut self) {
        let cr3 = Cr3::read();

        if cr3 == self.pml4_phys {
            Cr3::write(cr3);
        }
    }

    pub fn map_global_higher_half(&mut self) {
        unsafe {
            let k = get_kernel_page_table().lock();
            let k_pml4 = &mut *((k.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
            let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);

            // 0xFFFF_8000_0000_0000 - 0xFFFF_9000_0000_0000 (Kernel code)
            pml4.0[256..288].copy_from_slice(&k_pml4.0[256..288]);

            // 0xFFFF_9000_0000_0000 - 0xFFFF_A000_0000_0000 (Kernel stack)
            pml4.0[288..320].copy_from_slice(&k_pml4.0[288..320]);

            // 0xFFFF_A000_0000_0000 - 0xFFFF_B000_0000_0000 (Direct mapping)
            pml4.0[320..352].copy_from_slice(&k_pml4.0[320..352]);

            // 0xFFFF_B000_0000_0000 - 0xFFFF_C000_0000_0000 (MMIO)
            pml4.0[352..384].copy_from_slice(&k_pml4.0[352..384]);
        }
    }

    pub fn unmap_global_higher_half(&mut self) {
        unsafe {
            let pml4 = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);

            // 0xFFFF_8000_0000_0000 - 0xFFFF_9000_0000_0000 (Kernel code)
            pml4.0[256..288].fill(0);

            // 0xFFFF_9000_0000_0000 - 0xFFFF_A000_0000_0000 (Kernel stack)
            pml4.0[288..320].fill(0);

            // 0xFFFF_A000_0000_0000 - 0xFFFF_B000_0000_0000 (Direct mapping)
            pml4.0[320..352].fill(0);

            // 0xFFFF_B000_0000_0000 - 0xFFFF_C000_0000_0000 (MMIO)
            pml4.0[352..384].fill(0);
        }
    }

    pub fn translate(&mut self, virt: u64) -> Option<u64> {
        unsafe {
            let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = split_virt_addr(virt);
            println!(
                "Translating virt: {:#x}. PML4 idx: {}, PDPT idx: {}, PD idx: {}, PT idx: {}",
                virt, pml4_idx, pdpt_idx, pd_idx, pt_idx
            );

            let allocator = &mut *self.allocator;

            let pml4: &mut Table = &mut *((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut Table);
            println!("pml4[{}] = {:#x}", pml4_idx, pml4.0[pml4_idx]);
            let pdpt = pml4.get_table::<false>(pml4_idx, allocator, 0, 0)?;
            println!("pdpt[{}] = {:#x}", pdpt_idx, pdpt.0[pdpt_idx]);
            let pd = pdpt.get_table::<false>(pdpt_idx, allocator, 0, 0)?;
            println!("pd[{}] = {:#x}", pd_idx, pd.0[pd_idx]);

            let pd_entry = *pd.get_entry(pd_idx);
            if (pd_entry & PAGE_PRESENT) == PAGE_PRESENT && (pd_entry & PAGE_HUGE) == PAGE_HUGE {
                return Some((pd_entry & 0x000F_FFFF_FFFF_F000) + (virt % PAGE_SIZE_2MB as u64));
            }

            let pt = pd.get_table::<false>(pd_idx, allocator, 0, PAGE_HUGE)?;
            println!("pt[{}] = {:#x}", pt_idx, pt.0[pt_idx]);
            let pt_entry = *pt.get_entry(pt_idx);
            if (pt_entry & PAGE_PRESENT) == PAGE_PRESENT {
                return Some((pt_entry & 0x000F_FFFF_FFFF_F000) + (virt % PAGE_SIZE as u64));
            }

            None
        }
    }

    /// # Safety
    /// This function is unsafe because it modifies the CR3 register <br>
    /// Caller must make sure code is running in ring 0 and that the return address is mapped <br>
    pub unsafe fn load(&mut self) {
        Cr3::write(self.pml4_phys);
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        unsafe {
            if self.pml4_phys == 0 {
                return;
            }
            if Cr3::read() == self.pml4_phys {
                panic!("Dropping active page table");
            }

            let self_ptr = self as *mut PageTable;

            self.unmap_global_higher_half();
            for PageTableEntry {
                virt, page_size, ..
            } in self.iter_range(0, 0xFFFF_FFFF_FFFF_FFFF)
            {
                match page_size {
                    PageSize::Kb4 => (*self_ptr)
                        .unmap_4kb(virt, false)
                        .expect("Failed to unmap 4kb page"),
                    PageSize::Mb2 => (*self_ptr)
                        .unmap_2mb(virt, false)
                        .expect("Failed to unmap 2mb page"),
                    PageSize::Gb1 => unreachable!(),
                }
            }

            if self.allocator_owns_pml4 {
                (*self.allocator).free_page((self.pml4_phys + DIRECT_MAPPING_OFFSET) as *mut u8);
            }
            // prevent double free
            self.owns_allocator = false;

            if self.owns_allocator {
                dealloc(
                    self.allocator as *mut KernelPageTablesAllocator as *mut u8,
                    Layout::new::<KernelPageTablesAllocator>(),
                );
            }
            // prevent double free
            self.owns_allocator = false;
        }
    }
}

pub struct KernelPageTablesAllocator;

impl PageAllocator for KernelPageTablesAllocator {
    fn alloc_page(&mut self) -> Option<*mut u8> {
        let layout = Layout::from_size_align(4096, 4096).unwrap();
        let addr = unsafe { alloc(layout) };
        if addr.is_null() {
            None
        } else {
            Some(addr)
        }
    }

    fn free_page(&mut self, page: *mut u8) {
        let layout = Layout::from_size_align(4096, 4096).unwrap();
        unsafe { dealloc(page as u64 as *mut u8, layout) };
    }
}

pub fn run_on_global_kernel_stack<F>(f: F) -> !
where
    F: FnOnce(),
{
    unsafe {
        let kpt = get_kernel_page_table().lock();
        let cr3 = kpt.pml4_phys;
        let rsp = KERNEL_STACK_POINTER.get().unwrap();
        drop(kpt);

        core::arch::asm!(
            "mov rsp, {rsp}",
            "mov cr3, {cr3}",
            "push {f}",
            "ret",
            rsp = in(reg) rsp,
            cr3 = in(reg) cr3,
            f = in(reg) &f,
            options(noreturn)
        );
    }
}
