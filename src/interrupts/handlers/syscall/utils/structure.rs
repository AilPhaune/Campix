use core::marker::PhantomData;

use crate::{interrupts::handlers::syscall::utils::buffer::UserProcessBuffer, paging::PageTable};

pub struct UserProcessStructure<T: Sized> {
    pub buffer: UserProcessBuffer,
    pub phantom: PhantomData<T>,
}

impl<T: Sized> UserProcessStructure<T> {
    pub fn new(data: *mut T) -> Option<Self> {
        if !data.is_aligned() {
            return None;
        }
        Some(Self {
            buffer: UserProcessBuffer::new(data as *mut u8, size_of::<T>()),
            phantom: PhantomData,
        })
    }

    pub fn verify_fully_mapped(&self, page_table: &mut PageTable) -> Option<&T> {
        Some(unsafe { &*(self.buffer.verify_fully_mapped(page_table)?.as_ptr() as *const T) })
    }

    pub fn verify_fully_mapped_mut(&mut self, page_table: &mut PageTable) -> Option<&mut T> {
        Some(unsafe { &mut *(self.buffer.verify_fully_mapped(page_table)?.as_ptr() as *mut T) })
    }
}
