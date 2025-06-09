use core::cell::UnsafeCell;

use spin::Mutex;

#[derive(Debug, Default)]
pub struct AssignOnce<T> {
    value: UnsafeCell<Option<T>>,
    lock: Mutex<()>,
}

impl<T> AssignOnce<T> {
    pub const fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
            lock: Mutex::new(()),
        }
    }

    pub fn get(&self) -> Option<&T> {
        unsafe { self.value.as_ref_unchecked() }.as_ref()
    }

    pub fn set(&self, value: T) {
        let guard = self.lock.lock();
        unsafe {
            *self.value.as_mut_unchecked() = Some(value);
        }
        drop(guard);
    }
}

unsafe impl<T: Sync> Sync for AssignOnce<T> {}
