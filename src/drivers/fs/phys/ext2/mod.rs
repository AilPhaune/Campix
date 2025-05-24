use core::num::NonZeroUsize;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use balloc::BlockAllocator;
use blockgroup::{BlockGroupDescriptor, RawBlockGroupDescriptor, BLOCK_GROUP_DESCRIPTOR_SIZE};
use file::{Directory, FileHandle};
use inode::{Inode, InodeReadingLocation, InodeType, RawInode};
use lru::LruCache;
use spin::RwLock;
use superblock::{
    OptionalFeatures, ROFeature, ROFeatures, RequiredFeature, RequiredFeatures, Superblock,
    SUPERBLOCK_SIGNATURE,
};

use crate::{
    data::{alloc_boxed_slice, either::Either, file::File},
    drivers::vfs::{
        default_get_file_implementation, Arcrwb, BlockDevice, FileHandleAllocator, FileStat,
        FileSystem, FsSpecificFileData, SeekPosition, Vfs, VfsError, VfsFile, VfsFileKind,
        WeakArcrwb, OPEN_MODE_APPEND, OPEN_MODE_BINARY, OPEN_MODE_NO_RESIZE, OPEN_MODE_READ,
        OPEN_MODE_WRITE,
    },
};

pub mod balloc;
pub mod blockgroup;
pub mod file;
pub mod inode;
pub mod superblock;

#[derive(Debug)]
pub enum Ext2Error {
    BadSuperblockMagic(u16),
    BadSuperblock {
        reason: &'static str,
        superblock: Box<Superblock>,
    },
    BadBlockGroupDescriptorTableEntrySize(u32, u32),
    BadBlockGroupDescriptorTable,
    BadDeviceSize {
        expected: u64,
        actual: u64,
    },
    UnsupportedRequiredFeatures {
        supported: RequiredFeatures,
        required: RequiredFeatures,
        missing: RequiredFeatures,
    },
    BadInodeIndex(u32),
    BadReadingLocation(InodeReadingLocation),
}

impl From<Ext2Error> for VfsError {
    fn from(value: Ext2Error) -> Self {
        VfsError::DriverError(Box::new(value))
    }
}

#[derive(Debug)]
pub struct Ext2Volume {
    device: File,
    read_only: bool,
    superblock: Superblock,

    block_size: u32,
    sectors_per_block: u32,
    block_count: u32,

    block_group_count: u32,
    blocks_per_group: u32,
    block_group_descriptor_table: Vec<BlockGroupDescriptor>,

    inode_size: u16,
    inodes_per_block: u32,

    block_cache: RwLock<LruCache<u32, Box<[u8]>>>,
    group_block_bitmap_caches: LruCache<u32, BlockAllocator>,

    // VFS stuff
    root_dir_fs_data: Option<Arc<Ext2FsSpecificFileData>>,
    os_id: u64,
    parent_os_id: u64,
    root_fs: Option<WeakArcrwb<Vfs>>,
    mount_point: Option<VfsFile>,
    handles: FileHandleAllocator,
}

impl Ext2Volume {
    /// Implementation status:
    /// - ROFeatures::FileSize64: File inodes can use the higher 32bit of the size field
    //  TODO: SparseDescriptorTables
    pub const fn supported_ro_features() -> ROFeatures {
        *ROFeatures::empty()
            .set(ROFeature::FileSize64)
            .set(ROFeature::SparseDescriptorTables)
    }

    /// Implementation status:
    /// - None
    pub const fn supported_optional_features() -> OptionalFeatures {
        OptionalFeatures::empty()
    }

    /// Implementation status:
    //  TODO DirectoryEntriesHaveTypeField: Add support
    pub const fn supported_required_features() -> RequiredFeatures {
        *RequiredFeatures::empty().set(RequiredFeature::DirectoryEntriesHaveTypeField)
    }

    /// cache_size is in bytes, gets rounded up to the next integer multiple of the block size
    pub fn from_device(
        device: File,
        block_cache_size: NonZeroUsize,
        block_usage_bitmap_cache_size: NonZeroUsize,
    ) -> Result<Self, VfsError> {
        if (device.get_open_mode() & OPEN_MODE_BINARY) == 0
            || (device.get_open_mode() & OPEN_MODE_READ) == 0
            || (device.get_open_mode() & OPEN_MODE_APPEND) == OPEN_MODE_APPEND
        {
            return Err(VfsError::InvalidOpenMode);
        }
        let stats = device.stats()?;

        let superblock = Superblock::from_device(&device)?;

        if superblock.signature != SUPERBLOCK_SIGNATURE {
            return Err(Ext2Error::BadSuperblockMagic(superblock.signature).into());
        }
        let block_size = 1024u32 << superblock.log_block_size;
        let block_count = superblock.blocks_count;
        let sectors_per_block = block_size / 512;

        if stats.size != (block_size as u64) * (block_count as u64) {
            return Err(Ext2Error::BadDeviceSize {
                expected: (block_size as u64) * (block_count as u64),
                actual: stats.size,
            }
            .into());
        }

        let required_features = superblock.required_features;
        let ro_features = superblock.readonly_or_support_features;

        if (required_features & Self::supported_required_features()) != required_features {
            return Err(Ext2Error::UnsupportedRequiredFeatures {
                supported: Self::supported_required_features(),
                required: required_features,
                missing: required_features & !Self::supported_required_features(),
            }
            .into());
        }

        let read_only = (device.get_open_mode() & OPEN_MODE_WRITE) == 0
            || (ro_features & Self::supported_ro_features()) != ro_features;

        let block_group_count = Self::count_block_groups(&superblock)?;

        let inode_size = if superblock.major_version_level >= 1 {
            superblock.inode_struct_size
        } else {
            128
        };
        if inode_size == 0 {
            return Err(Ext2Error::BadSuperblock {
                reason: "inode_size == 0",
                superblock: Box::new(superblock),
            }
            .into());
        }

        let inodes_per_block = block_size / (inode_size as u32);
        if inodes_per_block == 0 {
            return Err(Ext2Error::BadSuperblock {
                reason: "inodes_per_block == 0",
                superblock: Box::new(superblock),
            }
            .into());
        }

        let blocks_per_group = superblock.blocks_per_group;
        if blocks_per_group == 0 {
            return Err(Ext2Error::BadSuperblock {
                reason: "blocks_per_group == 0",
                superblock: Box::new(superblock),
            }
            .into());
        }

        let block_lru = LruCache::new(
            NonZeroUsize::new(block_cache_size.get().div_ceil(block_size as usize)).unwrap(), // Guaranteed to be non-zero
        );

        let block_bitmaps_lru = LruCache::new(
            NonZeroUsize::new(block_usage_bitmap_cache_size.get().div_ceil(
                BlockAllocator::group_bitmap_size(blocks_per_group, block_size),
            ))
            .unwrap(), // Guaranteed to be non-zero
        );

        let mut ext2 = Self {
            device,
            read_only,
            superblock,
            block_size,
            sectors_per_block,
            block_count,
            block_group_count,
            blocks_per_group,
            block_group_descriptor_table: Vec::new(),
            inode_size,
            inodes_per_block,
            block_cache: RwLock::new(block_lru),
            group_block_bitmap_caches: block_bitmaps_lru,
            // VFS stuff
            root_dir_fs_data: None,
            os_id: 0,
            parent_os_id: 0,
            root_fs: None,
            mount_point: None,
            handles: FileHandleAllocator::default(),
        };

        ext2.read_block_group_descriptor_table()?;

        Ok(ext2)
    }

    pub fn get_superblock(&self) -> &Superblock {
        &self.superblock
    }

    fn read_block_group_descriptor_table(&mut self) -> Result<(), VfsError> {
        let entry_count = self.block_group_count;
        let table_size = entry_count * BLOCK_GROUP_DESCRIPTOR_SIZE;

        let mut table = alloc::vec![0u8; table_size as usize];
        let start_byte = if self.block_size == 1024 {
            2048
        } else {
            self.block_size as u64
        };

        self.device.seek(SeekPosition::FromStart(start_byte))?;
        self.device.read(&mut table)?;

        self.block_group_descriptor_table
            .reserve_exact(entry_count as usize);
        for i in 0..entry_count {
            self.block_group_descriptor_table.push(
                BlockGroupDescriptor::from_bytes(
                    &table[(i * BLOCK_GROUP_DESCRIPTOR_SIZE) as usize
                        ..((i + 1) * BLOCK_GROUP_DESCRIPTOR_SIZE) as usize],
                )
                .ok_or(Ext2Error::BadBlockGroupDescriptorTable)?,
            );
        }

        Ok(())
    }

    fn count_block_groups(superblock: &Superblock) -> Result<u32, Ext2Error> {
        let bpg = superblock.blocks_per_group;
        let ipg = superblock.inodes_per_group;
        if bpg == 0 || ipg == 0 {
            return Err(Ext2Error::BadSuperblock {
                reason: "blocks_per_group == 0 || inodes_per_group == 0",
                superblock: Box::new(superblock.clone()),
            });
        }
        let r1 = superblock.blocks_count.div_ceil(bpg);
        let r2 = superblock.inodes_count.div_ceil(ipg);
        if r1 != r2 {
            Err(Ext2Error::BadBlockGroupDescriptorTableEntrySize(r1, r2))
        } else {
            Ok(r1)
        }
    }

    fn get_inode_group(&self, inode: u32) -> u32 {
        (inode - 1) / self.superblock.inodes_per_group
    }

    fn get_inode_index_in_group(&self, inode: u32) -> u32 {
        (inode - 1) % self.superblock.inodes_per_group
    }

    pub fn get_inode(&self, inode: u32) -> Result<Inode, VfsError> {
        if inode == 0 || inode > self.superblock.inodes_count {
            Err(Ext2Error::BadInodeIndex(inode))?;
        }

        let group = self.get_inode_group(inode);
        let index = self.get_inode_index_in_group(inode);

        let block = self
            .block_group_descriptor_table
            .get(group as usize)
            .ok_or(Ext2Error::BadBlockGroupDescriptorTable)?
            .inode_table_block;

        let block_index = index / self.inodes_per_block;
        let offset_in_block = (index % self.inodes_per_block) * (self.inode_size as u32);

        let mut buffer = alloc::vec![0u8; self.block_size as usize];
        self.read_block((block + block_index) as u64, &mut buffer)?;

        Ok(Inode::from_raw(
            unsafe {
                (buffer.as_ptr().add(offset_in_block as usize) as *const RawInode).read_unaligned()
            },
            inode,
        ))
    }

    pub fn update_inode(&mut self, inode: &Inode) -> Result<(), VfsError> {
        let group = self.get_inode_group(inode.inode_i);
        let index = self.get_inode_index_in_group(inode.inode_i);

        let block = self
            .block_group_descriptor_table
            .get(group as usize)
            .ok_or(Ext2Error::BadBlockGroupDescriptorTable)?
            .inode_table_block;

        let block_index = index / self.inodes_per_block;
        let offset_in_block = (index % self.inodes_per_block) * (self.inode_size as u32);

        let mut buffer = alloc::vec![0u8; self.block_size as usize];
        self.read_block((block + block_index) as u64, &mut buffer)?;
        unsafe {
            (buffer.as_ptr().add(offset_in_block as usize) as *mut RawInode)
                .write_unaligned(inode.get_raw())
        };
        self.write_block((block + block_index) as u64, &buffer)?;

        Ok(())
    }

    pub fn get_file_handle(&mut self, inode: Inode, mode: u64) -> Result<FileHandle, VfsError> {
        FileHandle::new(self, inode, mode)
    }

    fn get_file_for_inode(&mut self, inode_i: u32, name: Vec<char>) -> Result<VfsFile, VfsError> {
        let inode = self.get_inode(inode_i)?;

        let size = inode.get_size(self);
        let (data, kind) = match inode.inode_type {
            InodeType::Directory => (
                Either::new_right(Directory::new(
                    self,
                    inode,
                    OPEN_MODE_BINARY | OPEN_MODE_READ,
                )?),
                VfsFileKind::Directory,
            ),
            InodeType::File => (Either::new_left(inode), VfsFileKind::File),
            _ => Err(VfsError::UnknownError)?,
        };

        Ok(VfsFile::new(
            kind,
            name,
            size,
            if inode_i == 2 {
                self.parent_os_id
            } else {
                self.os_id
            },
            self.os_id,
            Arc::new(Ext2FsSpecificFileData { value: data }),
        ))
    }

    pub fn get_block_group_descriptor(&self, group: u32) -> Option<BlockGroupDescriptor> {
        self.block_group_descriptor_table
            .get(group as usize)
            .cloned()
    }

    pub fn block_group_contains_metadata_backup(&self, group: u32) -> bool {
        #[inline(always)]
        fn is_pow(a: u32, b: u32) -> bool {
            let mut num = b;
            while a > num {
                num = match num.checked_mul(b) {
                    Some(v) => v,
                    None => return false,
                };
            }
            num == a
        }
        !self
            .superblock
            .get_ro_features()
            .has(ROFeature::SparseDescriptorTables)
            || (group <= 1 || is_pow(group, 3) || is_pow(group, 5) || is_pow(group, 7))
    }

    fn get_backup_groups(&self) -> Box<dyn Iterator<Item = u32>> {
        if self
            .superblock
            .get_ro_features()
            .has(ROFeature::SparseDescriptorTables)
        {
            if self.block_group_count <= 1 {
                return Box::new(0..=self.block_group_count);
            }
            let mut vec: Vec<u32> = alloc::vec![0, 1];
            let mut pow3: u32 = 1;
            let mut pow5: u32 = 1;
            let mut pow7: u32 = 1;
            loop {
                pow3 = match pow3.checked_mul(3) {
                    None => break,
                    Some(v) => {
                        if v > self.block_group_count {
                            break;
                        }
                        vec.push(v);
                        v
                    }
                }
            }
            loop {
                pow5 = match pow5.checked_mul(5) {
                    None => break,
                    Some(v) => {
                        if v > self.block_group_count {
                            break;
                        }
                        vec.push(v);
                        v
                    }
                }
            }
            loop {
                pow7 = match pow7.checked_mul(7) {
                    None => break,
                    Some(v) => {
                        if v > self.block_group_count {
                            break;
                        }
                        vec.push(v);
                        v
                    }
                }
            }
            Box::new(vec.into_iter())
        } else {
            return Box::new(0..self.block_group_descriptor_table.len() as u32);
        }
    }

    pub fn set_superblock(&mut self, superblock: Superblock) -> Result<(), VfsError> {
        if self.read_only {
            return Err(VfsError::ActionNotAllowed);
        }
        self.superblock = superblock.clone();

        let mut buffer = alloc::vec![0u8; size_of::<Superblock>()];
        unsafe {
            (buffer.as_mut_ptr() as *mut Superblock).write_unaligned(superblock);
        }

        self.device.seek(SeekPosition::FromStart(1024))?;
        self.device.write(&buffer)?;

        for backup_group in self.get_backup_groups().as_mut().skip(1) {
            let lba = (backup_group as u64) * (self.blocks_per_group as u64) + 1;
            self.device
                .seek(SeekPosition::FromStart(self.block_size as u64 * lba))?;
            self.device.write(&buffer)?;
        }

        Ok(())
    }

    pub fn set_block_group_descriptor(
        &mut self,
        group: u32,
        descriptor: BlockGroupDescriptor,
    ) -> Result<(), VfsError> {
        if self.read_only {
            return Err(VfsError::ActionNotAllowed);
        }
        self.block_group_descriptor_table[group as usize] = descriptor;

        let byte_index = (group as usize) * (BLOCK_GROUP_DESCRIPTOR_SIZE as usize);
        let block_index = byte_index / self.block_size as usize;
        let offset_in_block = byte_index % self.block_size as usize;

        let mut buffer = alloc::vec![0u8; self.block_size as usize];
        unsafe {
            for backup_group in self.get_backup_groups().as_mut() {
                let backup = (backup_group as u64) * (self.blocks_per_group as u64)
                    + (block_index as u64)
                    + 2; // Because superblock is at +1

                self.read_block(backup, &mut buffer)?;
                (buffer.as_mut_ptr().add(offset_in_block) as *mut RawBlockGroupDescriptor)
                    .write_unaligned(descriptor.to_raw());
                self.write_block(backup, &buffer)?;
            }
        }
        Ok(())
    }

    pub fn get_block_allocator_for_group<'a: 'b, 'b>(
        // explicit lifetime because we make some unsafe shit to bypass ass borrow checker who can't see that `self.get_block_group_descriptor(group)` does NOT try to borrow self immutably while it's being mutably borrowed because the mutable borrow either isn't used anymore, or has been returned
        &'a mut self,
        group: u32,
    ) -> Result<Option<&'b mut BlockAllocator>, VfsError> {
        if self.read_only {
            return Ok(None);
        }

        if let Some(cached) = self.group_block_bitmap_caches.get_mut(&group) {
            // Safe because we know that we're immediately returning the mutable reference, and self can't be used while it's borrowed
            return Ok(Some(unsafe { &mut *(cached as *mut BlockAllocator) }));
        }

        let Some(descriptor) = self.get_block_group_descriptor(group) else {
            return Ok(None);
        };

        let min_block_inclusive = group * self.blocks_per_group + 1;
        let max_block_exclusive =
            (min_block_inclusive + self.blocks_per_group).min(self.block_count);
        let blocks = max_block_exclusive - min_block_inclusive;

        let bitmap_begin_inclusive = descriptor.block_usage_bitmap;
        let bitmap_bytes = blocks.div_ceil(8);
        let bitmap_blocks = bitmap_bytes.div_ceil(self.block_size);
        let bitmap_end_exclusive = bitmap_begin_inclusive + bitmap_blocks;

        let mut allocator = BlockAllocator::new(
            min_block_inclusive,
            max_block_exclusive,
            bitmap_begin_inclusive,
            bitmap_end_exclusive,
            self.block_size,
            descriptor,
        );
        allocator.read_all(self)?;

        if let Some(evicted) = self.group_block_bitmap_caches.push(group, allocator) {
            self.handle_evicted_block_bitmap_cache(evicted.0, evicted.1)?;
        }

        Ok(self.group_block_bitmap_caches.get_mut(&group))
    }

    pub fn flush_block_bitmap_cache(&mut self, group: u32) -> Result<(), VfsError> {
        if let Some(mut allocator) = self.group_block_bitmap_caches.pop(&group) {
            let diff = *allocator.get_diff_usage();
            *allocator.get_diff_usage() = 0;

            let mut superblock = self.get_superblock().clone();
            superblock.unallocated_blocks = ((superblock.unallocated_blocks as i64) - diff) as u32;
            self.handle_evicted_block_bitmap_cache(group, allocator)?;
            self.set_superblock(superblock)?;
        }
        Ok(())
    }

    fn handle_evicted_block_bitmap_cache(
        &mut self,
        group: u32,
        mut allocator: BlockAllocator,
    ) -> Result<(), VfsError> {
        allocator.write_dirty(self)?;
        self.set_block_group_descriptor(group, allocator.consume())?;
        Ok(())
    }

    pub fn alloc_block_any(&mut self) -> Result<u32, VfsError> {
        for group in 1..self.block_group_count {
            if let Some(allocator) = self.get_block_allocator_for_group(group)? {
                if let Ok(block) = allocator.alloc_block() {
                    return Ok(block);
                }
            }
        }
        Err(VfsError::OutOfSpace)
    }
}

impl BlockDevice for Ext2Volume {
    fn flush(&mut self) -> Result<(), VfsError> {
        let groups = self
            .group_block_bitmap_caches
            .iter()
            .map(|(k, _)| *k)
            .collect::<Vec<_>>();

        let mut total_diff = 0;

        for group in groups {
            if let Some(mut allocator) = self.group_block_bitmap_caches.pop(&group) {
                let diff = *allocator.get_diff_usage();
                *allocator.get_diff_usage() = 0;

                total_diff += diff;
                self.handle_evicted_block_bitmap_cache(group, allocator)?;
            }
        }

        let mut superblock = self.get_superblock().clone();
        superblock.unallocated_blocks =
            ((superblock.unallocated_blocks as i64) - total_diff) as u32;
        self.set_superblock(superblock)?;

        self.device.flush()
    }

    fn get_generation(&self) -> u64 {
        0
    }

    fn get_block_size(&self) -> u64 {
        self.block_size as u64
    }

    fn get_block_count(&self) -> u64 {
        self.block_count as u64
    }

    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        if buf.len() < self.block_size as usize {
            return Err(VfsError::BadBufferSize);
        }
        if lba >= self.block_count as u64 {
            return Err(VfsError::OutOfBounds);
        }
        let lba32 = lba as u32;

        let mut wguard = self.block_cache.write();
        if let Some(cached) = wguard.get(&lba32) {
            buf.copy_from_slice(cached);
            return Ok(self.block_size as u64);
        }

        self.device
            .seek(SeekPosition::FromStart(self.block_size as u64 * lba))?;

        let mut slice = alloc_boxed_slice::<u8>(self.block_size as usize);
        let read = self.device.read(&mut slice)?;
        buf[0..read as usize].copy_from_slice(&slice[0..read as usize]);

        wguard.push(lba32, slice);

        Ok(read)
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<u64, VfsError> {
        if buf.len() < self.block_size as usize {
            return Err(VfsError::BadBufferSize);
        }
        if self.read_only {
            return Err(VfsError::ActionNotAllowed);
        }
        let mut wguard = self.block_cache.write();

        self.device
            .seek(SeekPosition::FromStart(self.block_size as u64 * lba))?;
        let written = self.device.write(&buf[0..self.block_size as usize])?;

        let lba32 = lba as u32;

        if let Some(cached) = wguard.get_mut(&lba32) {
            cached.copy_from_slice(&buf[0..written as usize]);
            return Ok(self.block_size as u64);
        }

        Ok(written)
    }
}

impl Drop for Ext2Volume {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[derive(Debug)]
pub struct Ext2FsSpecificFileData {
    pub value: Either<Inode, Directory>,
}

impl FsSpecificFileData for Ext2FsSpecificFileData {}

impl FileSystem for Ext2Volume {
    fn os_id(&mut self) -> u64 {
        self.os_id
    }

    fn fs_type(&mut self) -> String {
        "ext2".to_string()
    }

    fn host_block_device(&mut self) -> Option<Arcrwb<dyn BlockDevice>> {
        None
    }

    fn get_root(&mut self) -> Result<VfsFile, VfsError> {
        Ok(VfsFile::new(
            VfsFileKind::Directory,
            alloc::vec!['/'],
            0,
            self.parent_os_id,
            self.os_id,
            self.root_dir_fs_data
                .clone()
                .ok_or(VfsError::FileSystemNotMounted)?,
        ))
    }

    fn get_mount_point(&mut self) -> Result<Option<VfsFile>, VfsError> {
        Ok(Some(
            self.mount_point
                .clone()
                .ok_or(VfsError::FileSystemNotMounted)?,
        ))
    }

    fn get_child(&mut self, file: &VfsFile, child: &[char]) -> Result<VfsFile, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        let data = file.get_fs_specific_data();
        let data = (*data)
            .as_any()
            .downcast_ref::<Ext2FsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match &data.value {
            Either::A(_) => Err(VfsError::NotDirectory),
            Either::B(dir) => dir
                .entries
                .iter()
                .find(|e| e.has_name(child))
                .map(|e| self.get_file_for_inode(e.inode(), [file.name(), e.name()].concat()))
                .ok_or(VfsError::PathNotFound)?,
        }
    }

    fn list_children(&mut self, file: &VfsFile) -> Result<Vec<VfsFile>, VfsError> {
        if file.fs() != self.os_id() {
            return Err(VfsError::FileSystemMismatch);
        }
        let data = file.get_fs_specific_data();
        let data = (*data)
            .as_any()
            .downcast_ref::<Ext2FsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match &data.value {
            Either::A(_) => Err(VfsError::NotDirectory),
            Either::B(dir) => {
                let mut files = Vec::new();
                for e in dir.entries.iter() {
                    if e.has_name(&['.']) || e.has_name(&['.', '.']) {
                        continue;
                    }
                    files.push(self.get_file_for_inode(e.inode(), e.name().to_vec())?);
                }
                Ok(files)
            }
        }
    }

    default_get_file_implementation!();

    fn create_child(
        &mut self,
        _directory: &VfsFile,
        _name: &[char],
        _kind: VfsFileKind,
    ) -> Result<VfsFile, VfsError> {
        // TODO: Write support
        Err(VfsError::ActionNotAllowed)
    }

    fn on_mount(
        &mut self,
        mount_point: &VfsFile,
        os_id: u64,
        root_fs: WeakArcrwb<Vfs>,
    ) -> Result<VfsFile, VfsError> {
        self.mount_point = Some(mount_point.clone());
        self.root_fs = Some(root_fs);
        self.os_id = os_id;

        self.root_dir_fs_data = Some(Arc::new(Ext2FsSpecificFileData {
            value: Either::B(Directory::new(
                self,
                self.get_inode(2)?,
                OPEN_MODE_BINARY | OPEN_MODE_READ,
            )?),
        }));

        self.get_root()
    }

    fn on_pre_unmount(&mut self) -> Result<bool, VfsError> {
        for handle in self
            .handles
            .iter()
            .copied()
            .collect::<Vec<u64>>()
            .into_iter()
        {
            self.fflush(handle)?;
            self.fclose(handle)?;
        }
        Ok(true)
    }

    fn on_unmount(&mut self) -> Result<(), VfsError> {
        self.mount_point = None;
        self.root_fs = None;
        self.os_id = 0;
        self.flush()?;
        unsafe { self.device._close()? };
        Ok(())
    }

    fn get_vfs(&mut self) -> Result<WeakArcrwb<Vfs>, VfsError> {
        self.root_fs.clone().ok_or(VfsError::FileSystemNotMounted)
    }

    fn fopen(&mut self, file: &VfsFile, mode: u64) -> Result<u64, VfsError> {
        let data = file.get_fs_specific_data();
        let data = (*data)
            .as_any()
            .downcast_ref::<Ext2FsSpecificFileData>()
            .ok_or(VfsError::FileSystemMismatch)?;

        match &data.value {
            Either::A(inode) => match file.kind() {
                VfsFileKind::File => {
                    let handle = FileHandle::new(self, inode.clone(), mode)?;
                    Ok(self.handles.alloc_file_handle::<FileHandle>(handle))
                }
                _ => Err(VfsError::NotFile),
            },
            Either::B(_) => Err(VfsError::NotFile),
        }
    }

    fn fclose(&mut self, handle: u64) -> Result<(), VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        data.flush(self)?;

        self.handles.dealloc_file_handle::<FileHandle>(handle);
        Ok(())
    }

    fn fseek(&mut self, handle: u64, position: SeekPosition) -> Result<u64, VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        data.seek(self, position)?;
        Ok(data.get_position())
    }

    fn fread(&mut self, handle: u64, buf: &mut [u8]) -> Result<u64, VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        if data.get_open_mode() & OPEN_MODE_READ == 0 {
            return Err(VfsError::ActionNotAllowed);
        }
        data.read(self, buf)
    }

    fn fwrite(&mut self, handle: u64, buf: &[u8]) -> Result<u64, VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        if data.get_open_mode() & OPEN_MODE_WRITE == 0 {
            return Err(VfsError::ActionNotAllowed);
        }
        let checked_buf = if data.get_open_mode() & OPEN_MODE_NO_RESIZE == OPEN_MODE_NO_RESIZE {
            let pos = data.get_position();
            let max_pos = data.get_size();
            if pos > max_pos {
                return Err(VfsError::ActionNotAllowed);
            }
            &buf[0..(max_pos - pos) as usize]
        } else {
            let pos = data.get_position();
            let max_pos = data.get_size();
            let end = pos + buf.len() as u64;
            if end > max_pos {
                data.grow(self, end)?;
            }
            buf
        };
        data.write(self, checked_buf)
    }

    fn ftruncate(&mut self, handle: u64) -> Result<u64, VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        if data.get_open_mode() & OPEN_MODE_WRITE == 0 {
            return Err(VfsError::ActionNotAllowed);
        }
        if data.get_open_mode() & OPEN_MODE_NO_RESIZE == OPEN_MODE_NO_RESIZE {
            return Err(VfsError::ActionNotAllowed);
        }
        data.truncate(self, data.get_position())?;
        Ok(data.get_size())
    }

    fn fflush(&mut self, handle: u64) -> Result<(), VfsError> {
        let data = unsafe {
            &mut *self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        if data.get_open_mode() & OPEN_MODE_WRITE == 0 {
            return Err(VfsError::ActionNotAllowed);
        }
        data.flush(self)
    }

    fn fsync(&mut self, _handle: u64) -> Result<(), VfsError> {
        Err(VfsError::ActionNotAllowed)
    }

    fn fstat(&self, handle: u64) -> Result<FileStat, VfsError> {
        let data = unsafe {
            &*self
                .handles
                .get_handle_data::<FileHandle>(handle)
                .ok_or(VfsError::BadHandle)?
        };
        let inode = data.get_inode();
        Ok(FileStat {
            size: data.get_size(),
            // ext2 permissions are unix permissions, Campix kernel's permissions uses the same lower 16 bits as unix permissions
            permissions: inode.permissions.get() as u64,
            flags: 0,
            created_at: inode.ctime as u64,
            modified_at: inode.atime as u64,
            is_directory: false,
            is_symlink: false,
            owner_id: inode.uid as u64,
            group_id: inode.gid as u64,
        })
    }
}
