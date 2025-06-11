use alloc::vec::Vec;

use crate::{
    paging::{align_down, PageTable, PAGE_SIZE},
    process::memory::{get_address_space, VirtualAddressSpace},
};

pub struct UserProcessBuffer {
    pub buffer: *mut u8,
    pub size: usize,
}

impl UserProcessBuffer {
    pub fn new(buffer: *mut u8, size: usize) -> Self {
        Self { buffer, size }
    }

    fn verify_fully_mapped_impl(&self, page_table: &mut PageTable) -> Option<()> {
        let begin_addr = self.buffer as u64;
        let end_addr = (self.buffer as u64).checked_add(self.size as u64)?;

        let begin_page_addr = align_down(begin_addr, PAGE_SIZE as u64);
        let end_page_addr = align_down(end_addr, PAGE_SIZE as u64);

        let begin_space = get_address_space(begin_addr);
        let end_space = get_address_space(end_addr);

        if !matches!(
            (begin_space, end_space),
            (
                Some(VirtualAddressSpace::HigherHalf(..)),
                Some(VirtualAddressSpace::HigherHalf(..))
            ) | (
                Some(VirtualAddressSpace::LowerHalf(..)),
                Some(VirtualAddressSpace::LowerHalf(..))
            )
        ) {
            return None;
        }

        unsafe {
            let mut last: Option<u64> = None;
            for page in page_table.iter_range(begin_page_addr, end_page_addr + PAGE_SIZE as u64) {
                match last {
                    None => {
                        // first iteration, check for the first page
                        if page.virt != begin_page_addr {
                            return None;
                        }
                        last = Some(page.virt);
                    }
                    Some(last_addr) => {
                        // subsequent iteration, check for the next page
                        if page.virt != last_addr + PAGE_SIZE as u64 {
                            return None;
                        }
                        last = Some(page.virt);
                    }
                }
            }
            // at the end chek for the last page
            if last != Some(end_page_addr) {
                return None;
            }
        }

        Some(())
    }

    pub fn verify_fully_mapped(&self, page_table: &mut PageTable) -> Option<&[u8]> {
        self.verify_fully_mapped_impl(page_table)?;
        Some(unsafe { core::slice::from_raw_parts(self.buffer, self.size) })
    }

    pub fn verify_fully_mapped_mut<'a>(
        &'a mut self,
        page_table: &mut PageTable,
    ) -> Option<&'a mut [u8]> {
        self.verify_fully_mapped_impl(page_table)?;
        Some(unsafe { core::slice::from_raw_parts_mut(self.buffer, self.size) })
    }

    pub fn copy_user_c_str(
        page_table: &mut PageTable,
        addr: u64,
        max_len: u64,
    ) -> Option<(Vec<u8>, bool)> {
        let mut vec = Vec::new();

        let end_unaligned = addr + max_len;
        let end_aligned = align_down(end_unaligned, PAGE_SIZE as u64);

        let mut curr_addr = addr;

        while curr_addr < end_aligned {
            page_table.translate(curr_addr)?;

            let read = align_down(curr_addr + PAGE_SIZE as u64, PAGE_SIZE as u64)
                .min(end_unaligned)
                - curr_addr;

            let slice =
                unsafe { core::slice::from_raw_parts(curr_addr as *const u8, read as usize) };
            let idx_of_zero = slice.iter().position(|&x| x == 0).unwrap_or(read as usize);
            vec.extend_from_slice(&slice[..idx_of_zero]);
            if idx_of_zero < read as usize {
                return Some((vec, true));
            }

            curr_addr += read;
        }

        Some((vec, false))
    }
}
