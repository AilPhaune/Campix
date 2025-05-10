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
}

#[derive(Debug, Clone)]
pub struct FileReader {
    location: CachedInodeReadingLocation,
    offset: u64,
    size: u64,

    block_cache: Box<[u8]>,
    block_cache_info: Option<BlockCacheInfo>,
}

impl FileReader {
    pub fn new(volume: &Ext2Volume, inode: Inode) -> Result<Self, VfsError> {
        let bs = volume.get_block_size();
        let size = inode.get_size(volume);
        Ok(Self {
            location: CachedInodeReadingLocation::new(volume, inode)?,
            offset: 0,
            size,
            block_cache: alloc_boxed_slice::<u8>(bs as usize),
            block_cache_info: None,
        })
    }

    fn internal_update_buffer(&mut self, volume: &Ext2Volume) -> Result<(), VfsError> {
        match self.location.read_block(volume, &mut self.block_cache) {
            Ok(read) => {
                self.block_cache_info = Some(BlockCacheInfo {
                    block: self.location.current_block_idx(),
                    size: read as u32,
                });
                Ok(())
            }
            Err(e) => {
                self.block_cache_info = None;
                Err(e)
            }
        }
    }

    pub fn seek(&mut self, volume: &Ext2Volume, seek: SeekPosition) -> Result<(), VfsError> {
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

    pub fn read(
        &mut self,
        volume: &Ext2Volume,
        buffer: &mut [u8],
        max_count: u64,
    ) -> Result<u64, VfsError> {
        if max_count as usize > buffer.len() {
            return Err(VfsError::BadBufferSize);
        }
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
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct DirectoryEntryrRaw {
    inode: u32,
    entry_size: u16,
    len_lo: u8,
    type_or_len_hi: u8,
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    iode: u32,
    length: u32,
    name: Vec<char>,
}

impl DirectoryEntry {
    pub fn name(&self) -> &[char] {
        &self.name
    }

    pub fn inode(&self) -> u32 {
        self.iode
    }

    pub fn length(&self) -> u32 {
        self.length
    }

    pub fn has_name(&self, name: &[char]) -> bool {
        self.name == name
    }
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub entries: Vec<DirectoryEntry>,
    pub inode: Inode,
}

impl Directory {
    pub fn new(volume: &Ext2Volume, inode: Inode) -> Result<Self, VfsError> {
        let size = inode.get_size(volume) as usize;
        let mut entries = Vec::new();

        let mut buffer = alloc_boxed_slice::<u8>(size);
        let mut reader = FileReader::new(volume, inode)?;
        reader.read(volume, &mut buffer, size as u64)?;

        let mut idx = 0;
        while idx < size {
            let entry_raw =
                unsafe { (buffer.as_ptr().add(idx) as *const DirectoryEntryrRaw).read_unaligned() };

            let name_len = if volume
                .get_superblock()
                .get_required_features()
                .has(RequiredFeature::DirectoryEntriesHaveTypeField)
            {
                entry_raw.len_lo as usize
            } else {
                ((entry_raw.type_or_len_hi as usize) << 8) + (entry_raw.len_lo as usize)
            };

            let name_offset = idx + size_of::<DirectoryEntryrRaw>();
            let name = &buffer[name_offset..(name_offset + name_len)];

            if entry_raw.inode != 0 {
                entries.push(DirectoryEntry {
                    iode: entry_raw.inode,
                    length: entry_raw.entry_size as u32,
                    name: name.iter().map(|c| *c as char).collect(),
                });
            }
            idx += entry_raw.entry_size as usize;
        }

        let inode = reader.consume();
        Ok(Self { entries, inode })
    }
}
