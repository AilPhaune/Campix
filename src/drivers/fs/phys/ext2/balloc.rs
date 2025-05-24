use alloc::{boxed::Box, format};

use crate::{
    data::bitmap::Bitmap,
    drivers::vfs::{BlockDevice, VfsError},
};

use super::{blockgroup::BlockGroupDescriptor, Ext2Volume};

pub struct BlockAllocator {
    min_block_inclusive: u32,
    max_block_exclusive: u32,

    bitmap_begin_inclusive: u32,
    bitmap_end_exclusive: u32,

    bs: usize,

    bitmap: Bitmap,
    dirty_blocks_bitmap: Bitmap,

    descriptor: BlockGroupDescriptor,

    diff_usage: i64,
}

impl BlockAllocator {
    pub const fn group_bitmap_size(blocks_per_group: u32, block_size: u32) -> usize {
        let block_bits = block_size as usize * 8;
        let bitmap_bits = blocks_per_group as usize * block_bits;
        bitmap_bits.div_ceil(8)
    }

    pub fn new(
        min_block_inclusive: u32,
        max_block_exclusive: u32,
        bitmap_begin_inclusive: u32,
        bitmap_end_exclusive: u32,
        block_size: u32,
        descriptor: BlockGroupDescriptor,
    ) -> Self {
        Self {
            min_block_inclusive,
            max_block_exclusive,
            bitmap_begin_inclusive,
            bitmap_end_exclusive,
            bitmap: Bitmap::new(
                (bitmap_end_exclusive - bitmap_begin_inclusive) as usize
                    * (block_size as usize)
                    * 8, // in bits
            ),
            dirty_blocks_bitmap: Bitmap::new(
                (bitmap_end_exclusive - bitmap_begin_inclusive) as usize,
            ),
            bs: block_size as usize,
            descriptor,
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

    pub fn alloc_block(&mut self) -> Result<u32, VfsError> {
        match self.bitmap.find_first_unset() {
            Some(bit_index) => {
                self.bitmap.set_bit(bit_index, true);
                self.descriptor.free_blocks_count -= 1;
                self.diff_usage += 1;
                self.mark_dirty(bit_index);
                Ok(self.min_block_inclusive + bit_index as u32)
            }
            None => Err(VfsError::OutOfSpace),
        }
    }

    pub fn dealloc_block(&mut self, block: u32) -> Result<(), VfsError> {
        if block < self.min_block_inclusive || block >= self.max_block_exclusive {
            return Err(VfsError::InvalidArgument);
        }

        let bit_index = (block - self.min_block_inclusive) as usize;
        if !self.bitmap.get_bit(bit_index).unwrap_or(false) {
            // double free
            return Err(VfsError::DriverError(Box::new(format!(
                "Try to free already free block {block}"
            ))));
        }

        self.bitmap.set_bit(bit_index, false);
        self.descriptor.free_blocks_count += 1;
        self.diff_usage -= 1;
        self.mark_dirty(bit_index);
        Ok(())
    }

    fn mark_dirty(&mut self, bit_index: usize) {
        let block_index = bit_index / self.bs;
        self.dirty_blocks_bitmap.set_bit(block_index, true);
    }

    pub fn consume(self) -> BlockGroupDescriptor {
        self.descriptor
    }
}

impl Drop for BlockAllocator {
    fn drop(&mut self) {
        if self.dirty_blocks_bitmap.find_first_set().is_some() {
            panic!("Dropping block allocator with dirty blocks !!");
        }
    }
}
