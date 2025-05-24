use alloc::{boxed::Box, vec::Vec};

use crate::{
    data::alloc_boxed_slice,
    drivers::{
        fs::virt::devfs::fseek_helper,
        vfs::{BlockDevice, SeekPosition, VfsError},
    },
};

use super::{
    inode::{CachedInodeReadingLocation, Inode},
    superblock::RequiredFeature,
    Ext2Volume,
};

#[derive(Debug, Clone, Copy)]
struct BlockCacheInfo {
    block: u32,
    size: u32,
    dirty: bool,
}

#[derive(Debug, Clone)]
pub struct FileHandle {
    location: CachedInodeReadingLocation,
    open_mode: u64,
    offset: u64,
    size: u64,

    block_cache: Box<[u8]>,
    block_cache_info: Option<BlockCacheInfo>,
}

impl FileHandle {
    pub fn new(volume: &mut Ext2Volume, inode: Inode, open_mode: u64) -> Result<Self, VfsError> {
        let bs = volume.get_block_size();
        let size = inode.get_size(volume);
        Ok(Self {
            location: CachedInodeReadingLocation::new(volume, inode)?,
            offset: 0,
            size,
            block_cache: alloc_boxed_slice::<u8>(bs as usize),
            block_cache_info: None,
            open_mode,
        })
    }

    pub fn flush(&mut self, volume: &mut Ext2Volume) -> Result<(), VfsError> {
        if let Some(info) = &mut self.block_cache_info {
            if info.dirty {
                self.location.write_block(volume, &self.block_cache)?;
                info.dirty = false;
            }
        }
        Ok(())
    }

    fn dirty(&mut self) {
        if let Some(info) = &mut self.block_cache_info {
            info.dirty = true;
        }
    }

    fn internal_update_buffer(&mut self, volume: &mut Ext2Volume) -> Result<(), VfsError> {
        match self.location.read_block(volume, &mut self.block_cache) {
            Ok(read) => {
                self.block_cache_info = Some(BlockCacheInfo {
                    block: self.location.current_block_idx(),
                    size: read as u32,
                    dirty: false,
                });
                Ok(())
            }
            Err(e) => {
                self.block_cache_info = None;
                Err(e)
            }
        }
    }

    pub fn truncate(&mut self, volume: &mut Ext2Volume, new_size: u64) -> Result<(), VfsError> {
        if new_size > self.size {
            return Err(VfsError::InvalidArgument);
        }
        let bs = volume.get_block_size() as u32;
        let new_block_count: u32 = new_size
            .div_ceil(bs as u64)
            .try_into()
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;

        while self.location.block_count() > new_block_count {
            self.location.free_last_block(volume)?;
        }

        self.size = new_size;
        self.location.get_inode_mut().set_size(volume, new_size);
        volume.update_inode(self.get_inode())?;

        Ok(())
    }

    pub fn seek(&mut self, volume: &mut Ext2Volume, seek: SeekPosition) -> Result<(), VfsError> {
        let new_offset =
            fseek_helper(seek, self.offset, self.size).ok_or(VfsError::InvalidSeekPosition)?;

        let bs = volume.get_block_size();

        let block_offset: u32 = (new_offset / bs)
            .try_into()
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;

        self.offset = new_offset;
        self.location.seek(volume, block_offset)?;
        self.internal_update_buffer(volume)?;
        Ok(())
    }

    pub fn read(&mut self, volume: &mut Ext2Volume, buffer: &mut [u8]) -> Result<u64, VfsError> {
        let max_count = (buffer.len() as u64).min(self.size - self.offset);
        self.flush(volume)?;
        let bs = volume.get_block_size();
        let current_block = (self.offset / bs) as u32;
        let mut read = 0;
        if self.block_cache_info.is_none() {
            self.internal_update_buffer(volume)?;
        }

        if let Some(info) = self.block_cache_info {
            if current_block == info.block {
                let curr_off = self.offset % bs;
                let block_rem = bs - curr_off;
                let to_copy = max_count.min(block_rem);

                buffer[0..to_copy as usize].copy_from_slice(
                    &self.block_cache[curr_off as usize..(curr_off + to_copy) as usize],
                );
                read += to_copy;
                self.offset += to_copy;
            }

            while read < max_count {
                if !self.location.advance(volume)? {
                    break;
                }
                self.internal_update_buffer(volume)?;

                let rem_copy = (max_count - read).min(info.size as u64);
                buffer[read as usize..(read + rem_copy) as usize]
                    .copy_from_slice(&self.block_cache[0..rem_copy as usize]);
                read += rem_copy;
                self.offset += rem_copy;
            }
        }

        Ok(read)
    }

    pub fn write(&mut self, volume: &mut Ext2Volume, buffer: &[u8]) -> Result<u64, VfsError> {
        let bs = volume.get_block_size();
        let max_size = self.size.checked_next_multiple_of(bs).unwrap_or(self.size);
        let max_count = (buffer.len() as u64).min(max_size - self.offset);
        let begin_offset = self.offset;
        self.flush(volume)?;
        let current_block = (self.offset / bs) as u32;
        let mut written = 0;
        if self.block_cache_info.is_none() {
            self.internal_update_buffer(volume)?;
        }

        if let Some(info) = self.block_cache_info {
            if current_block == info.block {
                let curr_off = self.offset % bs;
                let block_rem = bs - curr_off;
                let to_copy = max_count.min(block_rem);

                self.block_cache[curr_off as usize..(curr_off + to_copy) as usize]
                    .copy_from_slice(&buffer[0..to_copy as usize]);
                written += to_copy;
                self.offset += to_copy;

                self.dirty();
            }

            while written < max_count {
                if !self.location.advance(volume)? {
                    break;
                }
                self.flush(volume)?;
                let rem_copy = (max_count - written).min(info.size as u64);
                if rem_copy != bs {
                    // If not writing a full block, we need to update the block cache
                    self.internal_update_buffer(volume)?;
                }

                self.block_cache[0..rem_copy as usize]
                    .copy_from_slice(&buffer[written as usize..(written + rem_copy) as usize]);
                written += rem_copy;
                self.offset += rem_copy;

                self.dirty();
            }
        }

        let new_size: u64 = self.size.max(begin_offset + written);
        if new_size != self.size {
            self.size = new_size;
            self.location.get_inode_mut().set_size(volume, new_size);
            volume.update_inode(self.get_inode())?;
        }

        Ok(written)
    }

    pub fn get_position(&self) -> u64 {
        self.offset
    }

    pub fn get_size(&self) -> u64 {
        self.size
    }

    pub fn is_eof(&self) -> bool {
        self.offset >= self.size
    }

    pub fn consume(self) -> Inode {
        self.location.consume()
    }

    pub fn get_inode(&self) -> &Inode {
        self.location.get_inode()
    }

    pub fn get_open_mode(&self) -> u64 {
        self.open_mode
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct DirectoryEntryRaw {
    inode: u32,
    entry_size: u16,
    len_lo: u8,
    type_or_len_hi: u8,
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    iode: u32,
    name: Vec<char>,
}

impl DirectoryEntry {
    pub fn name(&self) -> &[char] {
        &self.name
    }

    pub fn inode(&self) -> u32 {
        self.iode
    }

    pub fn has_name(&self, name: &[char]) -> bool {
        self.name == name
    }
}

pub struct DirectoryIterator<'a> {
    volume: &'a mut Ext2Volume,
    reader: FileHandle,
    size: usize,

    buffer: Box<[u8]>,
    buffer_idx: usize,
    idx: usize,
}

impl<'a> DirectoryIterator<'a> {
    pub fn new(volume: &'a mut Ext2Volume, inode: Inode, open_mode: u64) -> Result<Self, VfsError> {
        let size = inode.get_size(volume) as usize;
        let buffer = alloc_boxed_slice::<u8>(volume.block_size as usize);
        let reader = FileHandle::new(volume, inode, open_mode)?;
        Ok(Self {
            volume,
            reader,
            size,
            buffer,
            buffer_idx: usize::MAX,
            idx: 0,
        })
    }

    pub fn consume(self) -> Inode {
        self.reader.consume()
    }

    pub fn get_index(&self) -> usize {
        self.idx
    }
}

impl<'a> Iterator for DirectoryIterator<'a> {
    type Item = DirectoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.idx >= self.size {
                return None;
            }
            let buffer_idx = self.idx / self.volume.block_size as usize;
            let idx = self.idx % self.volume.block_size as usize;
            if buffer_idx != self.buffer_idx {
                self.reader.read(self.volume, &mut self.buffer).ok()?;
                self.buffer_idx = buffer_idx;
            }

            let entry_raw = unsafe {
                (self.buffer.as_ptr().add(self.idx) as *const DirectoryEntryRaw).read_unaligned()
            };

            let name_len = if self
                .volume
                .get_superblock()
                .get_required_features()
                .has(RequiredFeature::DirectoryEntriesHaveTypeField)
            {
                entry_raw.len_lo as usize
            } else {
                ((entry_raw.type_or_len_hi as usize) << 8) + (entry_raw.len_lo as usize)
            };

            let name_offset = idx + size_of::<DirectoryEntryRaw>();
            let name = &self.buffer[name_offset..(name_offset + name_len)];

            self.idx += entry_raw.entry_size as usize;
            if entry_raw.inode != 0 {
                return Some(DirectoryEntry {
                    iode: entry_raw.inode,
                    name: name.iter().map(|c| *c as char).collect(),
                });
            }
            if entry_raw.entry_size == 0 {
                return None;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub entries: Vec<DirectoryEntry>,
    pub inode: Inode,
}

impl Directory {
    pub fn new(volume: &mut Ext2Volume, inode: Inode, open_mode: u64) -> Result<Self, VfsError> {
        let iterator = DirectoryIterator::new(volume, inode.clone(), open_mode)?;
        Ok(Self {
            entries: iterator.collect(),
            inode,
        })
    }
}
