use core::fmt::Debug;

use alloc::boxed::Box;

use super::alloc_boxed_slice;

#[derive(Clone)]
pub struct Bitmap {
    data: Box<[u8]>,
    size: usize,
}

impl Bitmap {
    pub fn new(size: usize) -> Self {
        let mut data = Self {
            data: alloc_boxed_slice(size.div_ceil(8)),
            size,
        };
        data.clear();
        data
    }

    pub fn new_with_size_multiple(size: usize, multiple: usize) -> Self {
        let needed_size = size.div_ceil(8);
        let data_size = needed_size.next_multiple_of(multiple);
        let data = alloc_boxed_slice(data_size);
        Self { data, size }
    }

    pub fn new_with_data(size: usize, data: Box<[u8]>) -> Self {
        Self { data, size }
    }

    pub fn get_bit(&self, idx: usize) -> Option<bool> {
        if idx < self.size {
            let mask = 1 << (idx % 8);
            let idx = idx / 8;
            Some(self.data[idx] & mask != 0)
        } else {
            None
        }
    }

    pub fn set_bit(&mut self, idx: usize, enabled: bool) {
        if idx < self.size {
            let mask = 1 << (idx % 8);
            let idx = idx / 8;
            if enabled {
                self.data[idx] |= mask;
            } else {
                self.data[idx] &= !mask;
            }
        }
    }

    pub fn toggle_bit(&mut self, idx: usize) {
        if idx < self.size {
            let mask = 1 << (idx % 8);
            let idx = idx / 8;
            self.data[idx] ^= mask;
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn clear(&mut self) {
        self.data.fill(0);
    }

    pub fn find_first_unset(&self) -> Option<usize> {
        let mut bit_index = 0;

        let (head, body, tail) = unsafe { self.data.align_to::<usize>() };

        for &byte in head {
            if byte != 0xFF {
                for i in 0..8 {
                    if bit_index >= self.size {
                        return None;
                    }
                    if byte & (1 << i) == 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += 8;
            }
        }

        for &word in body {
            if word != usize::MAX {
                for i in 0..(usize::BITS as usize) {
                    if bit_index >= self.size {
                        return None;
                    }
                    if (word & (1 << i)) == 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += usize::BITS as usize;
            }
        }

        for &byte in tail {
            if byte != 0xFF {
                for i in 0..8 {
                    if bit_index >= self.size {
                        return None;
                    }
                    if byte & (1 << i) == 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += 8;
            }
        }

        None
    }

    pub fn find_first_set(&self) -> Option<usize> {
        let mut bit_index = 0;

        let (head, body, tail) = unsafe { self.data.align_to::<usize>() };

        for &byte in head {
            if byte != 0x00 {
                for i in 0..8 {
                    if bit_index >= self.size {
                        return None;
                    }
                    if byte & (1 << i) != 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += 8;
            }
        }

        for &word in body {
            if word != 0 {
                for i in 0..(usize::BITS as usize) {
                    if bit_index >= self.size {
                        return None;
                    }
                    if (word & (1 << i)) != 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += usize::BITS as usize;
            }
        }

        for &byte in tail {
            if byte != 0x00 {
                for i in 0..8 {
                    if bit_index >= self.size {
                        return None;
                    }
                    if byte & (1 << i) != 0 {
                        return Some(bit_index);
                    }
                    bit_index += 1;
                }
            } else {
                bit_index += 8;
            }
        }

        None
    }
}

impl Debug for Bitmap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Bitmap {{ size: {}, data: ", self.size)?;
        let mut bit = 0;
        while bit < self.size {
            let begin = bit;
            while self.get_bit(bit) == Some(true) {
                bit += 1;
            }
            if bit == begin + 1 {
                write!(f, "{begin},")?;
            }
            if bit > begin + 1 {
                write!(f, "{begin}-{},", bit - 1)?;
            }
            bit += 1;
        }
        write!(f, " }}")?;
        Ok(())
    }
}
