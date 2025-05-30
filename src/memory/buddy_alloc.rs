use core::ptr::NonNull;

pub const PAGE_SIZE: u64 = 4096;

// 2^28 * 4KiB = 1TiB
const MAX_ORDER: u64 = 28;

struct FreeBlock {
    next: Option<NonNull<FreeBlock>>,
    order: u64,
}

pub struct BuddyPageAllocator {
    base_addr: u64,
    page_count: u64,
    free_lists: [Option<NonNull<FreeBlock>>; MAX_ORDER as usize + 1],
    allocated_pages: u64,
}

/// Given the number of pages, find the smallest order that fits
fn required_order(num_pages: u64) -> u64 {
    let mut order = 0;
    let mut total_size = 1;

    while total_size < num_pages {
        total_size *= 2;
        order += 1;
    }

    order
}

/// Find the buddy address
fn find_buddy(addr: u64, order: u64) -> u64 {
    addr ^ (PAGE_SIZE << order)
}

impl BuddyPageAllocator {
    /// # Safety
    /// `base_addr` must be page aligned <br>
    /// `page_count` must be greater than 0 <br>
    /// The range `[base_addr, base_addr + page_count * PAGE_SIZE[` must be valid memory <br>
    pub unsafe fn new(base_addr: u64, page_count: u64) -> Self {
        assert!((base_addr & (PAGE_SIZE - 1)) == 0 && page_count > 0);
        let mut alloc = Self {
            base_addr,
            page_count,
            free_lists: [None; MAX_ORDER as usize + 1],
            allocated_pages: 0,
        };
        alloc.init();
        alloc
    }

    /// Initialize the free lists
    unsafe fn init(&mut self) {
        let mut curr_page = 0;
        for i in (0..=MAX_ORDER).rev() {
            if self.page_count - curr_page < (1 << i) {
                // Not enough pages for this order
                continue;
            }

            let free_block = self.base_addr + curr_page * PAGE_SIZE;
            let free_block_ptr = free_block as *mut FreeBlock;
            *free_block_ptr = FreeBlock {
                next: None,
                order: i,
            };
            self.free_lists[i as usize] = NonNull::new(free_block_ptr);

            curr_page += 1 << i;
        }
    }

    /// Add a free block to the free list
    unsafe fn add_free_block(&mut self, addr: u64, order: u64) {
        assert_eq!(
            (addr - self.base_addr) % (PAGE_SIZE << order),
            0,
            "Address not properly aligned"
        );

        let block_ptr = addr as *mut FreeBlock;
        (*block_ptr).next = self.free_lists[order as usize].take();
        (*block_ptr).order = order;
        self.free_lists[order as usize] = NonNull::new(block_ptr);
    }

    /// Removes a free block from the free list
    fn consume(&mut self, order: u64) -> Option<NonNull<FreeBlock>> {
        if let Some(block) = self.free_lists[order as usize].take() {
            self.free_lists[order as usize] = unsafe { block.as_ref().next };
            assert_eq!(order, unsafe { block.as_ref() }.order);
            Some(block)
        } else {
            None
        }
    }

    /// Allocate a block of at least `num_pages` pages.
    /// Returns (physical_address, order) if successful.
    pub fn alloc(&mut self, num_pages: u64) -> Option<(u64, u8)> {
        if num_pages == 0 {
            return None;
        }
        let order = required_order(num_pages);
        if order > MAX_ORDER {
            return None;
        }

        // Find a free block at >= order
        let mut current_order = order;
        while current_order <= MAX_ORDER && self.free_lists[current_order as usize].is_none() {
            current_order += 1;
        }
        if current_order > MAX_ORDER {
            return None;
        }

        // Split blocks down to the desired order
        while current_order > order {
            if let Some(block) = self.consume(current_order) {
                let block_addr = block.as_ptr() as *mut _ as u64;
                let half_size = PAGE_SIZE << (current_order - 1);

                let buddy1 = block_addr;
                let buddy2 = block_addr + half_size;

                unsafe {
                    // Is safe because we know that `buddy1` and `buddy2` are aligned and in valid range
                    self.add_free_block(buddy2, current_order - 1);
                    self.add_free_block(buddy1, current_order - 1);
                }
            }
            current_order -= 1;
        }

        // Now free_lists[order] should have a block
        if let Some(block) = self.consume(order) {
            let block_addr = block.as_ptr() as *mut _ as u64;

            self.allocated_pages += 1 << order;

            Some((block_addr, order as u8))
        } else {
            None
        }
    }

    /// Free a previously allocated block.
    /// addr must be 4 KiB aligned.
    pub fn free(&mut self, addr: u64, mut order: u64) {
        let mut addr = addr;

        self.allocated_pages -= 1 << order;

        loop {
            let buddy_addr = find_buddy(addr - self.base_addr, order) + self.base_addr;

            if !unsafe {
                // Safe because we know that `buddy_addr` is aligned and in valid range
                self.remove_free_block(buddy_addr, order)
            } {
                // Buddy not free, stop here
                unsafe {
                    // Safe because we know that `addr` is aligned and in valid range
                    self.add_free_block(addr, order);
                }
                break;
            }

            // Buddy found and removed, merge and continue
            addr = core::cmp::min(addr, buddy_addr);
            order += 1;
            if order > MAX_ORDER {
                break;
            }
        }
    }

    /// Try to remove a block at `addr` from the free list at `order`.
    /// Returns true if found and removed.
    unsafe fn remove_free_block(&mut self, addr: u64, order: u64) -> bool {
        let mut current = &mut self.free_lists[order as usize];

        while let Some(mut block) = *current {
            let block_addr = block.as_ptr() as *mut _ as u64;
            if block_addr == addr {
                *current = block.as_mut().next.take();
                return true;
            }
            current = &mut block.as_mut().next;
        }

        false
    }

    pub fn get_allocated_page_count(&self) -> u64 {
        self.allocated_pages
    }

    pub fn get_page_count(&self) -> u64 {
        self.page_count
    }

    pub fn get_free_page_count(&self) -> u64 {
        self.page_count - self.allocated_pages
    }

    pub fn get_base_addr(&self) -> u64 {
        self.base_addr
    }
}
