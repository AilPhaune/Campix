use core::fmt::Debug;

use crate::{
    data::file::File,
    debuggable_bitset_enum,
    drivers::vfs::{SeekPosition, VfsError},
};

pub const SUPERBLOCK_SIGNATURE: u16 = 0xEF53;

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum FsState {
    Clean = 1,
    Error = 2,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum OnErrorBehavior {
    Continue = 1,
    Remount = 2,
    Panic = 3,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum OsId {
    Linux = 0,
    GnuHurd = 1,
    Masix = 2,
    FreeBsd = 3,
    Lites = 4,
}

debuggable_bitset_enum!(
    u32,
    pub enum OptionalFeature {
        PreallocateBlocks = 1,
        AfsServerInodes = 2,
        FsJournal = 4,
        ExtendedInodeAttributes = 8,
        FsResizeSelfLarger = 16,
        UseHashIndex = 32,
    },
    OptionalFeatures
);

debuggable_bitset_enum!(
    u32,
    pub enum RequiredFeature {
        Compression = 1,
        DirectoryEntriesHaveTypeField = 2,
        FsNeedsToReplayJournal = 4,
        FsUsesJournalDevice = 8,
    },
    RequiredFeatures
);

debuggable_bitset_enum!(
    u32,
    pub enum ROFeature {
        SparseDescriptorTables = 1,
        FileSize64 = 2,
        DirectoryContentInBinaryTree = 4,
    },
    ROFeatures
);

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub su_reserved: u32,
    pub unallocated_blocks: u32,
    pub unallocated_inodes: u32,
    pub superblock_block: u32,
    pub log_block_size: u32,
    pub log_fragment_size: u32,
    pub blocks_per_group: u32,
    pub fragments_per_group: u32,
    pub inodes_per_group: u32,
    pub last_mount_time: u32,
    pub last_write_time: u32,
    pub mount_count_since_fsck: u16,
    pub max_mount_count_before_fsck: u16,
    pub signature: u16,
    pub fs_state: FsState,
    pub on_error_behavior: OnErrorBehavior,
    pub minor_version_level: u16,
    pub last_fsck_time: u32,
    pub fsck_interval: u32,
    pub os_id: OsId,
    pub major_version_level: u32,
    pub user_id_reserved_blocks: u16,
    pub group_id_reserved_blocks: u16,

    // Extended Superblock
    pub first_non_reserved_inode: u32,
    pub inode_struct_size: u16,
    pub this_block_group: u16,
    pub optional_features: OptionalFeatures,
    pub required_features: RequiredFeatures,
    pub readonly_or_support_features: ROFeatures,
    pub fs_id: [u8; 16],
    pub volume_name: [u8; 16],
    pub last_mount_path: [u8; 64],
    pub compression_algorithm: u32,

    // Performance Hints
    pub file_block_preallocate_count: u8,
    pub directory_block_preallocate_count: u8,
    pub padding0: [u8; 2],

    // Journaling
    pub journal_id: [u8; 16],
    pub journal_inode: u32,
    pub journal_device: u32,
    pub head_of_orphan_inode_list: u32,

    // Directory Indexing
    pub hash_seed: [u32; 4],
    pub hash_version: u8,
    pub padding1: [u8; 3],

    // Other options
    pub default_mount_options: u32,
    pub first_meta_bg: u32,
    // Remaining 760 unused bytes
}

impl Superblock {
    pub fn from_device(device: &File) -> Result<Superblock, VfsError> {
        let mut data = [0u8; 1024];
        device.seek(SeekPosition::FromStart(1024))?;
        device.read(&mut data)?;
        Ok(unsafe { (data.as_ptr() as *const Superblock).read_unaligned() })
    }

    pub fn get_ro_features(&self) -> ROFeatures {
        self.readonly_or_support_features
    }

    pub fn get_required_features(&self) -> RequiredFeatures {
        self.required_features
    }

    pub fn get_optional_features(&self) -> OptionalFeatures {
        self.optional_features
    }
}
