use core::alloc::Layout;

use alloc::{alloc::alloc, boxed::Box};

pub mod assign_once;
pub mod bitmap;
pub mod bitset_enum;
pub mod either;
pub mod file;
pub mod partition;
pub mod permissions;
pub mod regs;

pub fn alloc_boxed_slice<T>(count: usize) -> Box<[T]> {
    let layout = Layout::array::<T>(count.max(1)).unwrap();
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

pub fn calloc_boxed_slice<T: Default>(count: usize) -> Box<[T]> {
    let mut slice = alloc_boxed_slice(count);
    for item in slice.iter_mut() {
        *item = Default::default();
    }
    slice
}

pub fn decimal_chars_to_u64(chars: &[char]) -> Option<u64> {
    let mut result: u64 = 0;
    for &c in chars {
        if let Some(digit) = c.to_digit(10) {
            result = result.checked_mul(10)?.checked_add(digit as u64)?;
        } else {
            return None; // Not a digit
        }
    }
    Some(result)
}
