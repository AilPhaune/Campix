use core::cell::SyncUnsafeCell;

use alloc::{boxed::Box, format, vec::Vec};
use spin::rwlock::RwLock;

use crate::{
    data::{alloc_boxed_slice, calloc_boxed_slice, file::File},
    drivers::ports::parallel::ParallelPort,
    kpanic_no_log,
    paging::PAGE_SIZE,
};

pub enum KernelStdoutState {
    Uninitialized,
    FixedSizeBuffer {
        buffer: *mut u8,
        size: usize,
        pos: usize,
    },
    GrowableBuffer {
        past_buffers: Vec<Box<[u8]>>,
        current_buffer: Box<[u8]>,
        current_buffer_pos: usize,
    },
    PipeTo {
        file: File,
    },
}

impl KernelStdoutState {
    pub fn write_char_impl(&mut self, c: u8) {
        match self {
            KernelStdoutState::Uninitialized => {
                kpanic_no_log(b"kernel stdout not initialized");
            }
            KernelStdoutState::FixedSizeBuffer { buffer, size, pos } => {
                if *pos < *size {
                    unsafe {
                        *buffer.add(*pos) = c;
                    }
                    *pos += 1;
                } else {
                    kpanic_no_log(b"kernel stdout buffer overflow");
                }
            }
            KernelStdoutState::GrowableBuffer {
                past_buffers,
                current_buffer,
                current_buffer_pos,
            } => {
                if *current_buffer_pos < current_buffer.len() {
                    current_buffer[*current_buffer_pos] = c;
                } else {
                    let mut new_buf = calloc_boxed_slice(PAGE_SIZE);
                    new_buf[0] = c;
                    let filled_buf = core::mem::replace(current_buffer, new_buf);
                    past_buffers.push(filled_buf);
                    *current_buffer_pos = 1;
                }
            }
            KernelStdoutState::PipeTo { file } => match file.write(&[c]) {
                Ok(_) => {}
                Err(e) => {
                    kpanic_no_log(format!("Failed to write to pipe: {e:?}").as_bytes());
                }
            },
        }
    }
}

pub struct KernelStdout {
    state: RwLock<KernelStdoutState>,
}

impl KernelStdout {
    /// # Safety
    /// `buffer` must be a valid pointer to a buffer of size `size`, all current cached content will be lost
    pub unsafe fn unsafe_set_fixed_size_buffer(&mut self, buffer: *mut u8, size: usize) {
        let mut lock = self.state.write();

        if !matches!(*lock, KernelStdoutState::Uninitialized) {
            panic!("Invalid operation: switch kernel logger to fixed size buffer when logger is initialized");
        }

        *lock = KernelStdoutState::FixedSizeBuffer {
            buffer,
            size,
            pos: 0,
        };
    }

    pub fn switch_to_heap(&mut self) {
        let mut lock = self.state.write();

        match &*lock {
            KernelStdoutState::Uninitialized => {
                *lock = KernelStdoutState::GrowableBuffer {
                    past_buffers: Vec::new(),
                    current_buffer: calloc_boxed_slice(PAGE_SIZE),
                    current_buffer_pos: 0,
                };
            }
            KernelStdoutState::GrowableBuffer {..} => {}
            KernelStdoutState::PipeTo { .. } => panic!("Invalid operation: switch kernel logger to heap buffer when virtual file system is initialized"),
            KernelStdoutState::FixedSizeBuffer { buffer, size, pos } => {
                let mut buffers = Vec::new();

                let count = (*pos).min(*size).div_ceil(PAGE_SIZE);

                let mut rem = (*pos).min(*size);
                let mut pos = 0;

                for _ in 0..count {
                    let mut alloc_buffer = alloc_boxed_slice(PAGE_SIZE);
                    let copy = rem.min(PAGE_SIZE);
                    unsafe {
                        core::ptr::copy::<u8>(
                            (*buffer).add(pos),
                            alloc_buffer.as_mut_ptr(),
                            copy,
                        );
                        if copy < PAGE_SIZE {
                            alloc_buffer[copy..].fill(0);
                        }
                    }
                    pos += copy;
                    rem -= copy;
                    buffers.push(alloc_buffer);
                }

                if buffers.is_empty() {
                    *lock = KernelStdoutState::GrowableBuffer { past_buffers: buffers, current_buffer: calloc_boxed_slice(PAGE_SIZE), current_buffer_pos: 0 };
                } else {
                    let last_buf = buffers.pop().unwrap();
                    let pos = pos % PAGE_SIZE;

                    *lock = KernelStdoutState::GrowableBuffer { past_buffers: buffers, current_buffer: last_buf, current_buffer_pos: pos };
                }
            }
        }
    }

    pub fn switch_to_pipe(&mut self, mut file: File) {
        let mut lock = self.state.write();
        match &*lock {
            KernelStdoutState::Uninitialized => {}
            KernelStdoutState::FixedSizeBuffer { buffer, size, pos } => {
                match file.write(unsafe { core::slice::from_raw_parts(*buffer, (*size).min(*pos)) })
                {
                    Ok(_) => {}
                    Err(e) => {
                        kpanic_no_log(format!("Failed to write to pipe: {e:?}").as_bytes());
                    }
                }
            }
            KernelStdoutState::GrowableBuffer {
                past_buffers,
                current_buffer,
                current_buffer_pos,
            } => {
                for buf in past_buffers.iter() {
                    match file.write(buf) {
                        Ok(_) => {}
                        Err(e) => {
                            kpanic_no_log(format!("Failed to write to pipe: {e:?}").as_bytes());
                        }
                    }
                }
                if *current_buffer_pos > 0 {
                    match file.write(&current_buffer[..*current_buffer_pos]) {
                        Ok(_) => {}
                        Err(e) => {
                            kpanic_no_log(format!("Failed to write to pipe: {e:?}").as_bytes());
                        }
                    }
                }
            }
            KernelStdoutState::PipeTo { .. } => {}
        }
        *lock = KernelStdoutState::PipeTo { file };
    }

    pub fn panic_dump_to(&mut self, lpt: ParallelPort) {
        match self.state.get_mut() {
            KernelStdoutState::Uninitialized | KernelStdoutState::PipeTo { .. } => {}
            KernelStdoutState::FixedSizeBuffer { buffer, size, pos } => {
                for i in 0..(*pos).min(*size) {
                    unsafe { lpt.write_byte((*buffer).add(i).read_volatile()) };
                }
            }
            KernelStdoutState::GrowableBuffer {
                past_buffers,
                current_buffer,
                current_buffer_pos,
            } => {
                for buf in past_buffers.iter() {
                    for i in 0..buf.len() {
                        unsafe { lpt.write_byte(buf[i]) };
                    }
                }
                for i in 0..*current_buffer_pos {
                    unsafe { lpt.write_byte(current_buffer[i]) };
                }
            }
        }
    }
}

impl core::fmt::Write for KernelStdout {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut lock = self.state.write();

        for c in s.chars() {
            if c == '\n' {
                lock.write_char_impl(b'\r');
            }
            lock.write_char_impl(c as u8);
        }
        Ok(())
    }

    fn write_char(&mut self, c: char) -> core::fmt::Result {
        let mut lock = self.state.write();
        lock.write_char_impl(c as u8);
        Ok(())
    }

    fn write_fmt(&mut self, fmt: core::fmt::Arguments) -> core::fmt::Result {
        core::fmt::write(self, fmt)?;
        Ok(())
    }
}

unsafe impl Sync for KernelStdout {}

pub static KERNEL_STDOUT: SyncUnsafeCell<KernelStdout> = SyncUnsafeCell::new(KernelStdout {
    state: RwLock::new(KernelStdoutState::Uninitialized),
});

pub fn get_stdout() -> &'static mut KernelStdout {
    unsafe { &mut *KERNEL_STDOUT.get() }
}

#[macro_export]
macro_rules! printf {
    ($fmt: expr) => {{
        use core::fmt::Write;
        let writer = $crate::log::get_stdout();
        write!(writer, $fmt).unwrap();
    }};
    ($fmt: expr, $( $arg: expr ),*) => {{
        use core::fmt::Write;
        let writer = $crate::log::get_stdout();
        write!(writer, $fmt, $( $arg ),*).unwrap();
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::printf!("\n");
    }};
    ($fmt: expr) => {{
        use core::fmt::Write;
        let writer = $crate::log::get_stdout();
        write!(writer, $fmt).unwrap();
        write!(writer, "\n").unwrap();
    }};
    ($fmt: expr, $( $arg: expr ),*) => {{
        use core::fmt::Write;
        let writer = $crate::log::get_stdout();
        write!(writer, $fmt, $( $arg ),*).unwrap();
        write!(writer, "\n").unwrap();
    }};
}
