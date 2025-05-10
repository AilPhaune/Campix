use core::alloc::Layout;

use alloc::{alloc::alloc, boxed::Box};

pub mod bitset_enum;
pub mod either;
pub mod file;
pub mod partition;
pub mod permissions;

pub fn alloc_boxed_slice<T>(count: usize) -> Box<[T]> {
    let layout = Layout::array::<T>(count).unwrap();
    let ptr = unsafe { alloc(layout) as *mut T };
    if ptr.is_null() {
        panic!(
            "Failed to allocate memory for boxed slice of {} elements of type {}. Layout: {:#?}",
            count,
            core::any::type_name::<T>(),
            layout
        );
    }
    unsafe {
        let slice: *mut [T] = core::ptr::slice_from_raw_parts_mut(ptr, count);
        Box::from_raw(slice)
    }
}
