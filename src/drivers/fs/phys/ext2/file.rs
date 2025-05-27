use alloc::{boxed::Box, vec::Vec};

use crate::{
    data::alloc_boxed_slice,
    drivers::{
        fs::virt::devfs::fseek_helper,
        vfs::{BlockDevice, SeekPosition, VfsError, OPEN_MODE_BINARY, OPEN_MODE_WRITE},
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
        self.location.flush(volume)?;
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

        self.flush(volume)?;
        self.block_cache_info = None;
        self.seek(volume, SeekPosition::FromStart(new_size))?;

        Ok(())
    }

    pub fn grow(&mut self, volume: &mut Ext2Volume, new_size: u64) -> Result<(), VfsError> {
        if new_size < self.size {
            return Err(VfsError::InvalidArgument);
        }
        let curr_pos = self.offset;

        let bs = volume.get_block_size() as u32;
        let new_block_count: u32 = new_size
            .div_ceil(bs as u64)
            .try_into()
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;

        let mut diff_alloc = 0;
        while self.location.block_count() < new_block_count {
            let allocated_block_count = self.location.allocate_new_block(volume)?;
            diff_alloc += allocated_block_count;
        }

        self.size = new_size;
        let inode = self.location.get_inode_mut();
        inode.set_size(volume, new_size);
        inode.sectors_count += diff_alloc * volume.sectors_per_block;
        volume.update_inode(self.get_inode())?;

        self.flush(volume)?;
        self.block_cache_info = None;
        self.seek(volume, SeekPosition::FromStart(curr_pos))?;

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
                self.flush(volume)?;
                if !self.location.advance(volume)? {
                    break;
                }
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

#[repr(u8)]
pub enum DirectoryEntryType {
    Unknown = 0,
    File = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    BufferFile = 5,
    SocketFile = 6,
    Symlink = 7,
}

impl TryFrom<u8> for DirectoryEntryType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DirectoryEntryType::Unknown),
            1 => Ok(DirectoryEntryType::File),
            2 => Ok(DirectoryEntryType::Directory),
            3 => Ok(DirectoryEntryType::CharacterDevice),
            4 => Ok(DirectoryEntryType::BlockDevice),
            5 => Ok(DirectoryEntryType::BufferFile),
            6 => Ok(DirectoryEntryType::SocketFile),
            7 => Ok(DirectoryEntryType::Symlink),
            _ => Err(()),
        }
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
    inode: u32,
    name: Vec<char>,
}

impl DirectoryEntry {
    pub fn name(&self) -> &[char] {
        &self.name
    }

    pub fn inode(&self) -> u32 {
        self.inode
    }

    pub fn has_name(&self, name: &[char]) -> bool {
        self.name == name
    }
}

pub struct DirectoryIterator<'a> {
    volume: &'a mut Ext2Volume,
    handle: FileHandle,
    size: usize,

    buffer: Box<[u8]>,
    buffer_idx: usize,
    idx: usize,

    have_type_field: bool,
    last_entry_offset: Option<u64>,
}

impl<'a> DirectoryIterator<'a> {
    pub fn new(volume: &'a mut Ext2Volume, inode: Inode, open_mode: u64) -> Result<Self, VfsError> {
        let have_type_field = volume
            .get_superblock()
            .get_required_features()
            .has(RequiredFeature::DirectoryEntriesHaveTypeField);
        let size = inode.get_size(volume) as usize;
        let bs = volume.block_size as usize;
        if size % bs != 0 {
            return Err(VfsError::InvalidDataStructure);
        }
        let buffer = alloc_boxed_slice::<u8>(bs);
        let handle = FileHandle::new(volume, inode, open_mode)?;
        Ok(Self {
            volume,
            handle,
            size,
            buffer,
            buffer_idx: usize::MAX,
            idx: 0,
            have_type_field,
            last_entry_offset: None,
        })
    }

    pub fn consume(self) -> Inode {
        self.handle.consume()
    }

    pub fn get_index(&self) -> usize {
        self.idx
    }

    fn read_buffer(&mut self) -> Result<usize, VfsError> {
        let buffer_idx = self.idx / self.volume.block_size as usize;
        let idx = self.idx % self.volume.block_size as usize;
        if buffer_idx != self.buffer_idx {
            self.handle.read(self.volume, &mut self.buffer)?;
            self.buffer_idx = buffer_idx;
        }
        Ok(idx)
    }

    pub fn move_to_entry(&mut self, entry: &DirectoryIteratorEntry) -> Result<(), VfsError> {
        self.idx = entry.offset as usize;
        self.read_buffer()?;
        Ok(())
    }

    pub fn delete_entry(&mut self, entry: DirectoryIteratorEntry) -> Result<(), VfsError> {
        self.idx = entry.offset as usize;
        let idx = self.read_buffer()?;

        let mut entry_raw = unsafe {
            (self.buffer.as_ptr().add(self.idx) as *const DirectoryEntryRaw).read_unaligned()
        };

        let name_len = if self.have_type_field {
            entry_raw.len_lo as usize
        } else {
            ((entry_raw.type_or_len_hi as usize) << 8) + (entry_raw.len_lo as usize)
        };

        let name_offset = idx + size_of::<DirectoryEntryRaw>();

        self.buffer[name_offset..(name_offset + name_len)].fill(0);

        entry_raw.inode = 0;
        entry_raw.type_or_len_hi = 0;
        entry_raw.len_lo = 0;
        entry_raw.entry_size = if let Some(previous) = entry.prev_entry_offset {
            let previous_block = previous / self.volume.block_size as u64;
            if previous_block == self.buffer_idx as u64 {
                // We will coalesce
                0
            } else {
                // Don't touch
                entry_raw.entry_size
            }
        } else {
            // Don't touch
            entry_raw.entry_size
        };

        unsafe {
            (self.buffer.as_ptr().add(self.idx) as *mut DirectoryEntryRaw)
                .write_unaligned(entry_raw)
        };

        macro_rules! done {
            () => {
                let pos = self.buffer_idx as u64 * self.volume.block_size as u64;
                self.handle
                    .seek(self.volume, SeekPosition::FromStart(pos))?;
                self.handle.write(self.volume, &self.buffer)?;
                self.handle.flush(self.volume)?;

                self.idx = (entry.offset + entry.rec_len) as usize;
                self.read_buffer()?;
            };
        }

        let Some(previous) = entry.prev_entry_offset else {
            done!();
            return Ok(());
        };

        let previous_block = previous / self.volume.block_size as u64;
        if previous_block != self.buffer_idx as u64 {
            done!();
            return Ok(());
        }

        // coalesce
        self.idx = previous as usize;
        let idx = self.idx % self.volume.block_size as usize;

        let mut prev_entry_raw =
            unsafe { (self.buffer.as_ptr().add(idx) as *const DirectoryEntryRaw).read_unaligned() };

        if let Ok(rec_len) =
            u16::try_from(prev_entry_raw.entry_size as usize + entry.rec_len as usize)
        {
            prev_entry_raw.entry_size = rec_len;
        }

        unsafe {
            (self.buffer.as_ptr().add(idx) as *mut DirectoryEntryRaw)
                .write_unaligned(prev_entry_raw)
        };

        done!();
        Ok(())
    }

    pub fn insert_entry(
        &mut self,
        inode_i: u32,
        name: &[u8],
        entry_type: DirectoryEntryType,
    ) -> Result<DirectoryIteratorEntry, VfsError> {
        let raw_name_len = name.len();
        if raw_name_len > 255 {
            return Err(VfsError::NameTooLong);
        }
        // entries need to be 4 bytes aligned
        let name_len = raw_name_len.next_multiple_of(4);
        let needed_space = name_len + size_of::<DirectoryEntryRaw>();

        macro_rules! done {
            () => {
                let pos = self.buffer_idx as u64 * self.volume.block_size as u64;
                self.handle
                    .seek(self.volume, SeekPosition::FromStart(pos))?;
                self.handle.write(self.volume, &self.buffer)?;
                self.handle.flush(self.volume)?;
            };
        }

        // advance until we find a free slot
        while let Some(entry) = self.next() {
            if entry.entry.inode() == 0 && entry.rec_len >= needed_space as u64 {
                // reuse entry
                self.idx = entry.offset as usize;
                let idx = self.read_buffer()?;

                let mut entry_raw = unsafe {
                    (self.buffer.as_ptr().add(idx) as *const DirectoryEntryRaw).read_unaligned()
                };
                entry_raw.inode = inode_i;
                entry_raw.type_or_len_hi = if self.have_type_field {
                    entry_type as u8
                } else {
                    0
                };
                entry_raw.len_lo = raw_name_len as u8;

                let name_offset = idx + size_of::<DirectoryEntryRaw>();
                let name_buffer = &mut self.buffer[name_offset..(name_offset + raw_name_len)];
                name_buffer.copy_from_slice(name);

                unsafe {
                    (self.buffer.as_ptr().add(idx) as *mut DirectoryEntryRaw)
                        .write_unaligned(entry_raw)
                };

                done!();

                return self.next().ok_or(VfsError::UnknownError);
            }

            let entry_intrinsic_size =
                size_of::<DirectoryEntryRaw>() + entry.entry.name().len().next_multiple_of(4);
            let entry_free_size = entry.rec_len as usize - entry_intrinsic_size;

            if entry_free_size >= needed_space {
                // split entry
                self.idx = entry.offset as usize;
                let idx = self.read_buffer()?;

                let mut old_entry_raw = unsafe {
                    (self.buffer.as_ptr().add(idx) as *const DirectoryEntryRaw).read_unaligned()
                };

                old_entry_raw.entry_size = entry_intrinsic_size as u16;

                unsafe {
                    (self.buffer.as_ptr().add(idx) as *mut DirectoryEntryRaw)
                        .write_unaligned(old_entry_raw)
                };

                self.idx += entry_intrinsic_size;
                let idx = idx + entry_intrinsic_size;

                let mut entry_raw = unsafe {
                    (self.buffer.as_ptr().add(idx) as *const DirectoryEntryRaw).read_unaligned()
                };

                entry_raw.entry_size = entry_free_size as u16;
                entry_raw.inode = inode_i;
                entry_raw.type_or_len_hi = if self.have_type_field {
                    entry_type as u8
                } else {
                    0
                };
                entry_raw.len_lo = raw_name_len as u8;

                let name_offset = idx + size_of::<DirectoryEntryRaw>();
                let name_buffer = &mut self.buffer[name_offset..(name_offset + raw_name_len)];
                name_buffer.copy_from_slice(name);

                unsafe {
                    (self.buffer.as_ptr().add(idx) as *mut DirectoryEntryRaw)
                        .write_unaligned(entry_raw)
                };

                done!();

                return self.next().ok_or(VfsError::UnknownError);
            }
        }

        // No more space, need to allocate a new block
        let bs = self.volume.block_size as usize;
        if bs > u16::MAX as usize {
            return Err(VfsError::InvalidDataStructure);
        }
        if self.idx % bs != 0 {
            return Err(VfsError::InvalidDataStructure);
        }

        self.idx = self.size;
        self.size += bs;
        self.handle.grow(self.volume, self.size as u64)?;

        self.buffer_idx = self.idx / bs;
        self.buffer.fill(0);

        let entry_raw = DirectoryEntryRaw {
            entry_size: bs as u16,
            inode: inode_i,
            len_lo: raw_name_len as u8,
            type_or_len_hi: if self.have_type_field {
                entry_type as u8
            } else {
                0
            },
        };

        let name_offset = size_of::<DirectoryEntryRaw>();
        let name_buffer = &mut self.buffer[name_offset..(name_offset + raw_name_len)];
        name_buffer.copy_from_slice(name);

        unsafe { (self.buffer.as_ptr() as *mut DirectoryEntryRaw).write_unaligned(entry_raw) };

        done!();

        self.next().ok_or(VfsError::UnknownError)
    }
}

#[derive(Debug)]
pub struct DirectoryIteratorEntry {
    entry: DirectoryEntry,
    prev_entry_offset: Option<u64>,
    offset: u64,
    rec_len: u64,
}

impl<'a> Iterator for DirectoryIterator<'a> {
    type Item = DirectoryIteratorEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.idx >= self.size {
                return None;
            }
            let idx = self.read_buffer().ok()?;

            let entry_raw = unsafe {
                (self.buffer.as_ptr().add(self.idx) as *const DirectoryEntryRaw).read_unaligned()
            };

            let name_len = if self.have_type_field {
                entry_raw.len_lo as usize
            } else {
                ((entry_raw.type_or_len_hi as usize) << 8) + (entry_raw.len_lo as usize)
            };

            let name_offset = idx + size_of::<DirectoryEntryRaw>();
            let name = &self.buffer[name_offset..(name_offset + name_len)];

            let begin_offset = self.idx as u64;
            let rec_len = entry_raw.entry_size as u64;
            self.idx += rec_len as usize;
            if entry_raw.inode != 0 {
                let last_offset = self.last_entry_offset;
                self.last_entry_offset = Some(begin_offset);
                return Some(DirectoryIteratorEntry {
                    entry: DirectoryEntry {
                        inode: entry_raw.inode,
                        name: name.iter().map(|c| *c as char).collect(),
                    },
                    offset: begin_offset,
                    prev_entry_offset: last_offset,
                    rec_len,
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
            entries: iterator.map(|v| v.entry).collect(),
            inode,
        })
    }

    pub fn delete_entry(
        volume: &mut Ext2Volume,
        inode: &Inode,
        entry_inode: u32,
    ) -> Result<(), VfsError> {
        let mut iterator =
            DirectoryIterator::new(volume, inode.clone(), OPEN_MODE_BINARY | OPEN_MODE_WRITE)?;

        while let Some(next) = iterator.next() {
            if next.entry.inode == entry_inode {
                iterator.delete_entry(next)?;
                return Ok(());
            }
        }

        Err(VfsError::EntryNotFound)
    }
}
