pub const BLOCK_GROUP_DESCRIPTOR_SIZE: u32 = 32;

#[repr(C, packed)]
pub struct RawBlockGroupDescriptor {
    pub block_usage_bitmap: u32,
    pub inode_usage_bitmap: u32,
    pub inode_table_block: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub directory_count: u16,
    pub padding: [u8; 2],
    pub unused: [u8; 12],
}

#[derive(Debug, Clone, Copy)]
pub struct BlockGroupDescriptor {
    pub block_usage_bitmap: u32,
    pub inode_usage_bitmap: u32,
    pub inode_table_block: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub directory_count: u16,
}

impl BlockGroupDescriptor {
    const fn of(raw: RawBlockGroupDescriptor) -> Self {
        Self {
            block_usage_bitmap: raw.block_usage_bitmap,
            inode_usage_bitmap: raw.inode_usage_bitmap,
            inode_table_block: raw.inode_table_block,
            free_blocks_count: raw.free_blocks_count,
            free_inodes_count: raw.free_inodes_count,
            directory_count: raw.directory_count,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < BLOCK_GROUP_DESCRIPTOR_SIZE as usize {
            None
        } else {
            let raw =
                unsafe { (bytes.as_ptr() as *const RawBlockGroupDescriptor).read_unaligned() };
            Some(BlockGroupDescriptor::of(raw))
        }
    }

    pub fn to_raw(&self) -> RawBlockGroupDescriptor {
        RawBlockGroupDescriptor {
            block_usage_bitmap: self.block_usage_bitmap,
            inode_usage_bitmap: self.inode_usage_bitmap,
            inode_table_block: self.inode_table_block,
            free_blocks_count: self.free_blocks_count,
            free_inodes_count: self.free_inodes_count,
            directory_count: self.directory_count,
            padding: [0; 2],
            unused: [0; 12],
        }
    }
}
