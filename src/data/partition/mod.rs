use alloc::vec::Vec;

use crate::drivers::vfs::{Arcrwb, BlockDevice, VfsError};

use super::either::Either;

pub mod gpt;
pub mod mbr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockDeviceRange {
    /// The first sector of the range
    pub start: u64,
    /// The last sector of the range (not inclusive)
    pub end: u64,
}

#[derive(Debug, Clone)]
pub enum Partition {
    MBR(mbr::MBRPartition, BlockDeviceRange),
    GPT(gpt::GUIDPartitionTableEntry, BlockDeviceRange),
    Unknown(BlockDeviceRange),
}

impl Partition {
    pub fn as_device_range(&self) -> BlockDeviceRange {
        match self {
            Partition::MBR(_, range) => *range,
            Partition::GPT(_, range) => *range,
            Partition::Unknown(range) => *range,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PartitionScheme {
    None,
    MBR(mbr::MasterBootRecord),
    GPT(gpt::GUIDPartitionTable),
}

#[derive(Debug, Clone)]
pub struct PartitionManager {
    scheme: PartitionScheme,
    generation: u64,
}

impl Default for PartitionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PartitionManager {
    pub const fn new() -> Self {
        Self {
            scheme: PartitionScheme::None,
            generation: u64::MAX,
        }
    }

    pub fn get_partitions(&self) -> Vec<Partition> {
        match &self.scheme {
            PartitionScheme::None => Vec::new(),
            PartitionScheme::MBR(mbr) => mbr
                .partitions
                .iter()
                .filter(|p| !p.is_null())
                .map(|p| {
                    Partition::MBR(
                        *p,
                        BlockDeviceRange {
                            start: p.start_lba as u64,
                            end: p.start_lba as u64 + p.sector_count as u64,
                        },
                    )
                })
                .collect(),
            PartitionScheme::GPT(gpt) => gpt
                .get_partitions()
                .iter()
                .map(|p| Partition::GPT(p.clone(), p.as_device_range()))
                .collect(),
        }
    }

    pub fn get_partition(&self, index: usize) -> Option<Partition> {
        self.get_partitions().get(index).cloned()
    }

    pub fn get_generation(&self) -> u64 {
        self.generation
    }

    pub fn reload_partitions(&mut self, dev: Arcrwb<dyn BlockDevice>) -> Result<(), VfsError> {
        let guard = dev.read();
        self.generation = guard.get_generation();
        drop(guard);

        let partition_scheme = match gpt::GUIDPartitionTable::read(dev) {
            None => PartitionScheme::None,
            Some(Either::A(gpt)) => PartitionScheme::GPT(gpt),
            Some(Either::B(mbr)) => PartitionScheme::MBR(mbr),
        };

        self.scheme = partition_scheme;
        Ok(())
    }
}
