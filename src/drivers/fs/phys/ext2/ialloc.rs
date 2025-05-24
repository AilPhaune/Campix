use alloc::{boxed::Box, format};

use crate::{
    data::bitmap::Bitmap,
    drivers::vfs::{BlockDevice, VfsError},
};

use super::Ext2Volume;

pub struct InodeAllocator {
    min_inode_inclusive: u32,
    max_inode_exclusive: u32,

    bitmap_begin_inclusive: u32,
    bitmap_end_exclusive: u32,

    bs: usize,

    bitmap: Bitmap,
    dirty_blocks_bitmap: Bitmap,

    diff_usage: i64,
}

impl InodeAllocator {
    pub fn new(
        min_inode_inclusive: u32,
        max_inode_exclusive: u32,
        bitmap_begin_inclusive: u32,
        bitmap_end_exclusive: u32,
        block_size: u32,
    ) -> Self {
        let num_inodes = max_inode_exclusive - min_inode_inclusive;
        Self {
            min_inode_inclusive,
            max_inode_exclusive,
            bitmap_begin_inclusive,
            bitmap_end_exclusive,
            bs: block_size as usize,
            // Allocate num_inodes bits for inodes, but guarantee that the underlying slice is a multiple of block_size for convenience
            bitmap: Bitmap::new_with_size_multiple(num_inodes as usize, block_size as usize),
            // 8*block_size is the number of bits per block
            dirty_blocks_bitmap: Bitmap::new(num_inodes.div_ceil(8 * block_size) as usize),
            diff_usage: 0,
        }
    }

    pub fn get_diff_usage(&mut self) -> &mut i64 {
        &mut self.diff_usage
    }

    pub fn read_all(&mut self, volume: &mut Ext2Volume) -> Result<(), VfsError> {
        let slice = self.bitmap.as_mut_slice();
        for (i, lba) in (self.bitmap_begin_inclusive..self.bitmap_end_exclusive).enumerate() {
            volume.read_block(lba as u64, &mut slice[i * self.bs..(i + 1) * self.bs])?;
        }
        self.dirty_blocks_bitmap.clear();
        Ok(())
    }

    pub fn write_dirty(&mut self, volume: &mut Ext2Volume) -> Result<(), VfsError> {
        for (i, lba) in (self.bitmap_begin_inclusive..self.bitmap_end_exclusive).enumerate() {
            if self.dirty_blocks_bitmap.get_bit(i).unwrap_or(false) {
                volume.write_block(
                    lba as u64,
                    &self.bitmap.as_slice()[i * self.bs..(i + 1) * self.bs],
                )?;
                self.dirty_blocks_bitmap.toggle_bit(i);
            }
        }
        Ok(())
    }

    pub fn alloc_inode(&mut self) -> Result<u32, VfsError> {
        match self.bitmap.find_first_unset() {
            Some(bit_index) => {
                self.bitmap.set_bit(bit_index, true);
                self.diff_usage += 1;
                self.mark_dirty(bit_index);
                Ok(self.min_inode_inclusive + bit_index as u32)
            }
            None => Err(VfsError::OutOfSpace),
        }
    }

    pub fn dealloc_inode(&mut self, inode: u32) -> Result<(), VfsError> {
        if inode < self.min_inode_inclusive || inode >= self.max_inode_exclusive {
            return Err(VfsError::InvalidArgument);
        }

        let bit_index = (inode - self.min_inode_inclusive) as usize;
        if !self.bitmap.get_bit(bit_index).unwrap_or(false) {
            return Err(VfsError::DriverError(Box::new(format!(
                "Try to free already free inode {inode}"
            ))));
        }

        self.bitmap.set_bit(bit_index, false);
        self.diff_usage -= 1;
        self.mark_dirty(bit_index);
        Ok(())
    }

    fn mark_dirty(&mut self, bit_index: usize) {
        let block_index = bit_index / (self.bs * 8);
        self.dirty_blocks_bitmap.set_bit(block_index, true);
    }
}

impl Drop for InodeAllocator {
    fn drop(&mut self) {
        if self.dirty_blocks_bitmap.find_first_set().is_some() {
            panic!("Dropping inode allocator with dirty blocks !!");
        }
    }
}
