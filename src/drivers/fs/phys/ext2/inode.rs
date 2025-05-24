use alloc::{boxed::Box, format};

use crate::{
    data::alloc_boxed_slice,
    debuggable_bitset_enum,
    drivers::vfs::{BlockDevice, VfsError},
};

use super::{superblock::ROFeature, Ext2Error, Ext2Volume};

#[repr(C, packed)]
pub struct RawInode {
    pub type_and_permissions: u16,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub sectors_count: u32,
    pub flags: u32,
    pub ossv1: u32,
    pub direct_block_pointers: [u32; 12],
    pub single_indirect_block_pointer: u32,
    pub double_indirect_block_pointer: u32,
    pub triple_indirect_block_pointer: u32,
    pub generation_number: u32,
    pub extended_attribute_block: u32,
    pub size_hi_or_dir_acl: u32,
    pub fragment_block: u32,
    pub ossv2: [u8; 12],
}

#[derive(Debug, Clone)]
pub struct Inode {
    pub inode_type: InodeType,
    pub permissions: InodePermissions,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub sectors_count: u32,
    pub flags: InodeFlags,
    pub ossv1: u32,
    pub direct_block_pointers: [u32; 12],
    pub single_indirect_block_pointer: u32,
    pub double_indirect_block_pointer: u32,
    pub triple_indirect_block_pointer: u32,
    pub generation_number: u32,
    pub extended_attribute_block: u32,
    pub size_hi_or_dir_acl: u32,
    pub fragment_block: u32,
    pub ossv2: [u8; 12],

    // The inode number
    pub inode_i: u32,
}

impl Inode {
    pub const fn from_raw(raw_inode: RawInode, inode_i: u32) -> Self {
        Self {
            inode_type: unsafe {
                core::mem::transmute::<u16, InodeType>(raw_inode.type_and_permissions & 0xF000)
            },
            permissions: unsafe {
                core::mem::transmute::<u16, InodePermissions>(
                    raw_inode.type_and_permissions & 0x0FFF,
                )
            },
            uid: raw_inode.uid,
            size_lo: raw_inode.size_lo,
            atime: raw_inode.atime,
            ctime: raw_inode.ctime,
            mtime: raw_inode.mtime,
            dtime: raw_inode.dtime,
            gid: raw_inode.gid,
            links_count: raw_inode.links_count,
            sectors_count: raw_inode.sectors_count,
            flags: unsafe { core::mem::transmute::<u32, InodeFlags>(raw_inode.flags) },
            ossv1: raw_inode.ossv1,
            direct_block_pointers: raw_inode.direct_block_pointers,
            single_indirect_block_pointer: raw_inode.single_indirect_block_pointer,
            double_indirect_block_pointer: raw_inode.double_indirect_block_pointer,
            triple_indirect_block_pointer: raw_inode.triple_indirect_block_pointer,
            generation_number: raw_inode.generation_number,
            extended_attribute_block: raw_inode.extended_attribute_block,
            size_hi_or_dir_acl: raw_inode.size_hi_or_dir_acl,
            fragment_block: raw_inode.fragment_block,
            ossv2: raw_inode.ossv2,

            inode_i,
        }
    }

    pub fn get_raw(&self) -> RawInode {
        RawInode {
            type_and_permissions: self.inode_type as u16 | self.permissions.get(),
            uid: self.uid,
            size_lo: self.size_lo,
            atime: self.atime,
            ctime: self.ctime,
            mtime: self.mtime,
            dtime: self.dtime,
            gid: self.gid,
            links_count: self.links_count,
            sectors_count: self.sectors_count,
            flags: self.flags.get(),
            ossv1: self.ossv1,
            direct_block_pointers: self.direct_block_pointers,
            single_indirect_block_pointer: self.single_indirect_block_pointer,
            double_indirect_block_pointer: self.double_indirect_block_pointer,
            triple_indirect_block_pointer: self.triple_indirect_block_pointer,
            generation_number: self.generation_number,
            extended_attribute_block: self.extended_attribute_block,
            size_hi_or_dir_acl: self.size_hi_or_dir_acl,
            fragment_block: self.fragment_block,
            ossv2: self.ossv2,
        }
    }

    pub fn get_size(&self, volume: &Ext2Volume) -> u64 {
        if self.inode_type == InodeType::File
            && volume
                .get_superblock()
                .get_ro_features()
                .has(ROFeature::FileSize64)
        {
            ((self.size_hi_or_dir_acl as u64) << 32) | self.size_lo as u64
        } else {
            self.size_lo as u64
        }
    }

    pub fn set_size(&mut self, volume: &Ext2Volume, size: u64) {
        if self.inode_type == InodeType::File
            && volume
                .get_superblock()
                .get_ro_features()
                .has(ROFeature::FileSize64)
        {
            self.size_hi_or_dir_acl = (size >> 32) as u32;
        }
        self.size_lo = size as u32;
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    FIFO = 0x1000,
    CharacterDevice = 0x2000,
    Directory = 0x4000,
    BlockDevice = 0x6000,
    File = 0x8000,
    Symlink = 0xA000,
    Socket = 0xC000,
}

debuggable_bitset_enum!(
    u16,
    pub enum InodePermission {
        OtherExecute = 0o0001,
        OtherWrite = 0o0002,
        OtherRead = 0o0004,
        GroupExecute = 0o0010,
        GroupWrite = 0o0020,
        GroupRead = 0o0040,
        OwnerExecute = 0o0100,
        OwnerWrite = 0o0200,
        OwnerRead = 0o0400,
        StickyBit = 0o1000,
        SetGroupID = 0o2000,
        SetUserID = 0o4000,
    },
    InodePermissions
);

debuggable_bitset_enum!(
    u32,
    pub enum InodeFlag {
        SecureDeletion = 1,
        KeepCopyOfDataWhenDeleted = 2,
        FileCompression = 4,
        Synchronous = 8,
        Immutable = 16,
        AppendOnly = 32,
        NoDump = 64,
        NoUpdateATime = 128,
        // For compression
        Dirty = 256,
        CompressedBlocks = 512,
        NoCompress = 1024,
        CompressionError = 2048,
        // End of compression flags
        HashIndexedDirectory = 4096,
        AfsDirectory = 8192,
        JournalFileData = 16384,
    },
    InodeFlags
);

#[derive(Debug, Clone, Copy)]
pub enum InodeReadingLocationInfo {
    Direct(u32),
    Single(u32),
    Double(u32, u32),
    Triple(u32, u32, u32),
}

#[derive(Debug, Clone, Copy)]
pub struct InodeReadingLocation {
    location: InodeReadingLocationInfo,
    table_size: u32,
}

impl InodeReadingLocation {
    pub fn new(table_size: u32, block_idx: u32) -> Self {
        let table_size2 = table_size * table_size;

        let location = if block_idx < 12 {
            InodeReadingLocationInfo::Direct(block_idx)
        } else {
            let idx = block_idx - 12;
            if idx < table_size {
                InodeReadingLocationInfo::Single(idx)
            } else {
                let idx = idx - table_size;
                if idx < table_size2 {
                    let idx1 = idx / table_size;
                    let idx2 = idx % table_size;

                    InodeReadingLocationInfo::Double(idx1, idx2)
                } else {
                    let idx = idx - table_size2;
                    let idx1 = idx / table_size2;
                    let idx2 = (idx % table_size2) / table_size;
                    let idx3 = idx % table_size;

                    InodeReadingLocationInfo::Triple(idx1, idx2, idx3)
                }
            }
        };

        Self {
            table_size,
            location,
        }
    }

    pub fn current_block_idx(&self) -> u32 {
        if let InodeReadingLocationInfo::Direct(direct) = self.location {
            return direct;
        }
        let mut idx = 12;
        if let InodeReadingLocationInfo::Single(single) = self.location {
            return idx + single;
        }
        idx += self.table_size;
        if let InodeReadingLocationInfo::Double(double1, double2) = self.location {
            return idx + double1 * self.table_size + double2;
        }
        idx += self.table_size * self.table_size;
        if let InodeReadingLocationInfo::Triple(triple1, triple2, triple3) = self.location {
            return idx
                + triple1 * self.table_size * self.table_size
                + triple2 * self.table_size
                + triple3;
        }
        unreachable!();
    }

    pub fn advance(&mut self) -> bool {
        match self.location {
            InodeReadingLocationInfo::Direct(direct) => {
                if direct == 11 {
                    self.location = InodeReadingLocationInfo::Single(0);
                } else {
                    self.location = InodeReadingLocationInfo::Direct(direct + 1);
                }
            }
            InodeReadingLocationInfo::Single(single) => {
                if single == self.table_size - 1 {
                    self.location = InodeReadingLocationInfo::Double(0, 0);
                } else {
                    self.location = InodeReadingLocationInfo::Single(single + 1);
                }
            }
            InodeReadingLocationInfo::Double(double1, double2) => {
                if double1 == self.table_size - 1 && double2 == self.table_size - 1 {
                    self.location = InodeReadingLocationInfo::Triple(0, 0, 0);
                } else if double2 == self.table_size - 1 {
                    self.location = InodeReadingLocationInfo::Double(double1 + 1, 0);
                } else {
                    self.location = InodeReadingLocationInfo::Double(double1, double2 + 1);
                }
            }
            InodeReadingLocationInfo::Triple(triple1, triple2, triple3) => {
                if triple1 == self.table_size - 1
                    && triple2 == self.table_size - 1
                    && triple3 == self.table_size - 1
                {
                    return false;
                } else if triple2 == self.table_size - 1 && triple3 == self.table_size - 1 {
                    self.location = InodeReadingLocationInfo::Triple(triple1 + 1, 0, 0);
                } else if triple3 == self.table_size - 1 {
                    self.location = InodeReadingLocationInfo::Triple(triple1, triple2 + 1, 0);
                } else {
                    self.location = InodeReadingLocationInfo::Triple(triple1, triple2, triple3 + 1);
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct CachedInodeReadingLocation {
    location: InodeReadingLocation,
    inode: Inode,
    max_block_exclusive: i64,
    block_size: u64,

    table1: Box<[u8]>,
    table1_addr: u32,
    table1_dirty: bool,

    table2: Box<[u8]>,
    table2_addr: u32,
    table2_dirty: bool,

    table3: Box<[u8]>,
    table3_addr: u32,
    table3_dirty: bool,

    inode_dirty: bool,
}

impl CachedInodeReadingLocation {
    pub fn new(ext2: &Ext2Volume, inode: Inode) -> Result<Self, VfsError> {
        let size = ext2.get_block_size();
        let location = InodeReadingLocation::new(ext2.get_block_size() as u32 / 4, 0);
        let table1 = alloc_boxed_slice::<u8>(size as usize);
        let table2 = alloc_boxed_slice::<u8>(size as usize);
        let table3 = alloc_boxed_slice::<u8>(size as usize);

        let max_block_exclusive: i64 = inode
            .get_size(ext2)
            .div_ceil(size)
            .try_into()
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;

        Ok(Self {
            location,
            inode,
            max_block_exclusive,
            table1_addr: 0,
            table2_addr: 0,
            table3_addr: 0,
            table1,
            table2,
            table3,
            table1_dirty: false,
            table2_dirty: false,
            table3_dirty: false,
            block_size: size,
            inode_dirty: false,
        })
    }

    fn save_if_addresses_differ(
        ext2: &mut Ext2Volume,
        table: &[u8],
        current_addr: u32,
        to_load_addr: u32,
        dirty: &mut bool,
    ) -> Result<(), VfsError> {
        if *dirty && current_addr != 0 && current_addr != to_load_addr {
            if ext2.write_block(current_addr as u64, table)? != ext2.block_size as u64 {
                return Err(VfsError::UnknownError);
            }
            *dirty = false;
        };
        Ok(())
    }

    fn check_table1(&mut self, ext2: &mut Ext2Volume) -> Result<(), VfsError> {
        let addr = match self.location.location {
            InodeReadingLocationInfo::Direct(_) => 0,
            InodeReadingLocationInfo::Single(_) => self.inode.single_indirect_block_pointer,
            InodeReadingLocationInfo::Double(_, _) => self.inode.double_indirect_block_pointer,
            InodeReadingLocationInfo::Triple(_, _, _) => self.inode.triple_indirect_block_pointer,
        };

        Self::save_if_addresses_differ(
            ext2,
            &self.table1,
            self.table1_addr,
            addr,
            &mut self.table1_dirty,
        )?;

        if addr == 0 {
            self.table1_addr = 0;
            return Ok(());
        }

        if self.table1_addr != addr {
            match ext2.read_block(addr as u64, &mut self.table1) {
                Ok(_) => {
                    self.table1_addr = addr;
                }
                Err(e) => {
                    self.table1_addr = 0;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn follow1(&self, idx: u32) -> Result<u32, VfsError> {
        if idx as usize * 4 < self.table1.len() {
            let entry = unsafe { *(self.table1.as_ptr().add(idx as usize * 4) as *const u32) };
            Ok(entry)
        } else {
            Err(VfsError::OutOfBounds)
        }
    }

    fn check_table2(&mut self, ext2: &mut Ext2Volume) -> Result<(), VfsError> {
        let addr = match self.location.location {
            InodeReadingLocationInfo::Direct(_) => 0,
            InodeReadingLocationInfo::Single(_) => 0,
            InodeReadingLocationInfo::Double(p1, _)
            | InodeReadingLocationInfo::Triple(p1, _, _) => self.follow1(p1)?,
        };

        Self::save_if_addresses_differ(
            ext2,
            &self.table2,
            self.table2_addr,
            addr,
            &mut self.table2_dirty,
        )?;

        if addr == 0 {
            self.table2_addr = 0;
            return Ok(());
        }

        if self.table2_addr != addr {
            match ext2.read_block(addr as u64, &mut self.table2) {
                Ok(_) => {
                    self.table2_addr = addr;
                }
                Err(e) => {
                    self.table2_addr = 0;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn follow2(&self, idx: u32) -> Result<u32, VfsError> {
        if idx as usize * 4 < self.table2.len() {
            let entry = unsafe { *(self.table2.as_ptr().add(idx as usize * 4) as *const u32) };
            Ok(entry)
        } else {
            Err(VfsError::OutOfBounds)
        }
    }

    fn check_table3(&mut self, ext2: &mut Ext2Volume) -> Result<(), VfsError> {
        let addr = match self.location.location {
            InodeReadingLocationInfo::Direct(_) => 0,
            InodeReadingLocationInfo::Single(_) => 0,
            InodeReadingLocationInfo::Double(_, p2)
            | InodeReadingLocationInfo::Triple(_, p2, _) => self.follow2(p2)?,
        };

        Self::save_if_addresses_differ(
            ext2,
            &self.table3,
            self.table3_addr,
            addr,
            &mut self.table3_dirty,
        )?;

        if addr == 0 {
            self.table3_addr = 0;
            return Ok(());
        }

        if self.table3_addr != addr {
            match ext2.read_block(addr as u64, &mut self.table3) {
                Ok(_) => {
                    self.table3_addr = addr;
                }
                Err(e) => {
                    self.table3_addr = 0;
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn follow3(&self, idx: u32) -> Result<u32, VfsError> {
        if idx as usize * 4 < self.table3.len() {
            let entry = unsafe { *(self.table3.as_ptr().add(idx as usize * 4) as *const u32) };
            Ok(entry)
        } else {
            Err(VfsError::OutOfBounds)
        }
    }

    pub fn seek(&mut self, ext2: &mut Ext2Volume, block: u32) -> Result<(), VfsError> {
        self.location = InodeReadingLocation::new(ext2.get_block_size() as u32 / 4, block);
        self.check_table1(ext2)?;
        self.check_table2(ext2)?;
        self.check_table3(ext2)?;
        Ok(())
    }

    pub fn get_next_block(&self) -> Result<u32, VfsError> {
        Ok(match self.location.location {
            InodeReadingLocationInfo::Direct(direct) => {
                if direct >= 12 {
                    return Err(Ext2Error::BadReadingLocation(self.location).into());
                }
                self.inode.direct_block_pointers[direct as usize]
            }
            InodeReadingLocationInfo::Single(single) => self.follow1(single)?,
            InodeReadingLocationInfo::Double(_, double) => self.follow2(double)?,
            InodeReadingLocationInfo::Triple(_, _, triple) => self.follow3(triple)?,
        })
    }

    pub fn read_block(&mut self, ext2: &Ext2Volume, buffer: &mut [u8]) -> Result<u64, VfsError> {
        let bs = ext2.get_block_size();
        if buffer.len() < bs as usize {
            return Err(VfsError::BadBufferSize);
        }
        let block = self.get_next_block()?;
        let block_idx = self.location.current_block_idx();
        ext2.read_block(block as u64, buffer)?;
        if (block_idx as i64) < self.max_block_exclusive - 1 {
            Ok(bs)
        } else {
            let read = (self.inode.size_lo as u64) % bs;
            Ok(if read == 0 { bs } else { read })
        }
    }

    pub fn write_block(&mut self, ext2: &mut Ext2Volume, buffer: &[u8]) -> Result<u64, VfsError> {
        let bs = ext2.get_block_size();
        if buffer.len() < bs as usize {
            return Err(VfsError::BadBufferSize);
        }
        let block = self.get_next_block()?;
        let block_idx = self.location.current_block_idx();
        ext2.write_block(block as u64, buffer)?;
        if (block_idx as i64) < self.max_block_exclusive - 1 {
            Ok(bs)
        } else {
            let write = (self.inode.size_lo as u64) % bs;
            Ok(if write == 0 { bs } else { write })
        }
    }

    pub fn advance(&mut self, ext2: &mut Ext2Volume) -> Result<bool, VfsError> {
        let block = self.location.current_block_idx();
        if block as i64 >= self.max_block_exclusive - 1 || !self.location.advance() {
            return Ok(false);
        }
        self.check_table1(ext2)?;
        self.check_table2(ext2)?;
        self.check_table3(ext2)?;
        Ok(true)
    }

    pub fn current_block_idx(&self) -> u32 {
        self.location.current_block_idx()
    }

    pub fn consume(self) -> Inode {
        self.inode
    }

    pub fn get_inode(&self) -> &Inode {
        &self.inode
    }

    pub fn get_inode_mut(&mut self) -> &mut Inode {
        &mut self.inode
    }

    pub fn update(&mut self, volume: &Ext2Volume) -> Result<(), VfsError> {
        let max_block_exclusive: i64 = self
            .inode
            .get_size(volume)
            .div_ceil(self.block_size)
            .try_into()
            .map_err(|e| VfsError::DriverError(Box::new(e)))?;
        self.max_block_exclusive = max_block_exclusive;
        Ok(())
    }

    pub fn block_count(&self) -> u32 {
        self.max_block_exclusive as u32
    }

    pub fn free_last_block(&mut self, ext2: &mut Ext2Volume) -> Result<(), VfsError> {
        if self.max_block_exclusive == 0 {
            return Ok(());
        }
        self.seek(ext2, self.max_block_exclusive as u32 - 1)?;

        let block = self.get_next_block()?;
        let group = (block - 1) / ext2.blocks_per_group;

        let balloc = ext2
            .get_block_allocator_for_group(group)?
            .ok_or(VfsError::DriverError(Box::new(format!(
                "No block allocator for group {group}"
            ))))?;
        balloc.dealloc_block(block)?;
        unsafe {
            match self.location.location {
                InodeReadingLocationInfo::Direct(direct) => {
                    self.inode.direct_block_pointers[direct as usize] = 0;
                    self.inode_dirty = true;
                }
                InodeReadingLocationInfo::Single(idx0) => {
                    *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) = 0;
                    self.table1_dirty = true;
                    if idx0 == 0 {
                        self.inode.single_indirect_block_pointer = 0;
                        self.inode_dirty = true;
                        balloc.dealloc_block(self.table1_addr)?;
                    }
                }
                InodeReadingLocationInfo::Double(idx0, idx1) => {
                    *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) = 0;
                    self.table2_dirty = true;
                    if idx1 == 0 {
                        balloc.dealloc_block(
                            *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize),
                        )?;
                        *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) = 0;
                        self.table1_dirty = true;
                        if idx0 == 0 {
                            self.inode.double_indirect_block_pointer = 0;
                            self.inode_dirty = true;
                            balloc.dealloc_block(self.table1_addr)?;
                        }
                    }
                }
                InodeReadingLocationInfo::Triple(idx0, idx1, idx2) => {
                    *(self.table3.as_mut_ptr() as *mut u32).add(idx2 as usize) = 0;
                    self.table3_dirty = true;
                    if idx2 == 0 {
                        balloc.dealloc_block(
                            *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize),
                        )?;
                        *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) = 0;
                        self.table2_dirty = true;
                        if idx1 == 0 {
                            balloc.dealloc_block(
                                *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize),
                            )?;
                            *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) = 0;
                            self.table1_dirty = true;
                            if idx0 == 0 {
                                self.inode.triple_indirect_block_pointer = 0;
                                self.inode_dirty = true;
                                balloc.dealloc_block(self.table1_addr)?;
                            }
                        }
                    }
                }
            }
        }

        self.max_block_exclusive -= 1;
        Ok(())
    }

    pub fn allocate_new_block(&mut self, ext2: &mut Ext2Volume) -> Result<u32, VfsError> {
        let mut group = if self.max_block_exclusive == 0 {
            self.seek(ext2, 0)?;
            0
        } else {
            self.seek(ext2, self.max_block_exclusive as u32 - 1)?;

            let block = self.get_next_block()?;
            let group = (block - 1) / ext2.blocks_per_group;

            if !self.location.advance() {
                return Err(VfsError::MaximumSizeReached);
            }
            group
        };

        let current_sector_count = self.block_count() * ext2.sectors_per_block;
        let max_next_count = current_sector_count as u64 + 4 * ext2.sectors_per_block as u64;
        if max_next_count > u32::MAX as u64 {
            return Err(VfsError::MaximumSizeReached);
        }

        let mut alloc_count = 0;
        fn balloc(
            ext2: &mut Ext2Volume,
            group: &mut u32,
            alloc_count: &mut u32,
        ) -> Result<u32, VfsError> {
            let balloc =
                ext2.get_block_allocator_for_group(*group)?
                    .ok_or(VfsError::DriverError(Box::new(format!(
                        "No block allocator for group {group}"
                    ))))?;
            match balloc.alloc_block() {
                Err(_) => {
                    let block = ext2.alloc_block_any()?;
                    *group = (block - 1) / ext2.blocks_per_group;
                    *alloc_count += 1;
                    Ok(block)
                }
                Ok(b) => {
                    *alloc_count += 1;
                    Ok(b)
                }
            }
        }

        match self.location.location {
            InodeReadingLocationInfo::Direct(direct) => {
                if self.inode.direct_block_pointers[direct as usize] == 0 {
                    self.inode.direct_block_pointers[direct as usize] =
                        balloc(ext2, &mut group, &mut alloc_count)?;
                    self.inode_dirty = true;
                }
            }
            InodeReadingLocationInfo::Single(idx0) => {
                if self.inode.single_indirect_block_pointer == 0 {
                    self.inode.single_indirect_block_pointer =
                        balloc(ext2, &mut group, &mut alloc_count)?;
                    self.inode_dirty = true;
                    self.check_table1(ext2)?;
                }
                unsafe {
                    if *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) == 0 {
                        *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table1_dirty = true;
                    }
                }
            }
            InodeReadingLocationInfo::Double(idx0, idx1) => {
                if self.inode.double_indirect_block_pointer == 0 {
                    self.inode.double_indirect_block_pointer =
                        balloc(ext2, &mut group, &mut alloc_count)?;
                    self.inode_dirty = true;
                    self.check_table1(ext2)?;
                }
                unsafe {
                    if *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) == 0 {
                        *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table1_dirty = true;
                        self.check_table2(ext2)?;
                    }
                    if *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) == 0 {
                        *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table2_dirty = true;
                    }
                }
            }
            InodeReadingLocationInfo::Triple(idx0, idx1, idx2) => {
                if self.inode.triple_indirect_block_pointer == 0 {
                    self.inode.triple_indirect_block_pointer =
                        balloc(ext2, &mut group, &mut alloc_count)?;
                    self.inode_dirty = true;
                    self.check_table1(ext2)?;
                }
                unsafe {
                    if *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) == 0 {
                        *(self.table1.as_mut_ptr() as *mut u32).add(idx0 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table1_dirty = true;
                        self.check_table2(ext2)?;
                    }
                    if *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) == 0 {
                        *(self.table2.as_mut_ptr() as *mut u32).add(idx1 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table2_dirty = true;
                        self.check_table3(ext2)?;
                    }
                    if *(self.table3.as_mut_ptr() as *mut u32).add(idx2 as usize) == 0 {
                        *(self.table3.as_mut_ptr() as *mut u32).add(idx2 as usize) =
                            balloc(ext2, &mut group, &mut alloc_count)?;
                        self.table3_dirty = true;
                    }
                }
            }
        }
        self.max_block_exclusive += 1;

        Ok(alloc_count)
    }

    pub fn flush(&mut self, ext2: &mut Ext2Volume) -> Result<(), VfsError> {
        if self.table1_dirty && self.table1_addr != 0 {
            ext2.write_block(self.table1_addr as u64, &self.table1)?;
            self.table1_dirty = false;
        }
        if self.table2_dirty && self.table2_addr != 0 {
            ext2.write_block(self.table2_addr as u64, &self.table2)?;
            self.table2_dirty = false;
        }
        if self.table3_dirty && self.table3_addr != 0 {
            ext2.write_block(self.table3_addr as u64, &self.table3)?;
            self.table3_dirty = false;
        }
        if self.inode_dirty {
            ext2.update_inode(&self.inode)?;
            self.inode_dirty = false;
        }
        Ok(())
    }
}
