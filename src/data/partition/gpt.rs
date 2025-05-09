use alloc::vec::Vec;

use crate::{
    data::either::Either,
    drivers::vfs::{Arcrwb, BlockDevice, BlockDeviceAsCharacterDevice, CharacterDevice},
};

use super::{mbr::MasterBootRecord, BlockDeviceRange};

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct GPTHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub reserved: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_table_lba: u64,
    pub partition_entry_count: u32,
    pub partition_entry_size: u32,
    pub partition_entries_crc32: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
struct GUIDPartitionTableEntryRaw {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub flags: u64,
}

#[derive(Debug, Clone)]
pub struct GUIDPartitionTableEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub flags: u64,
    pub name: Vec<char>,
}

impl GUIDPartitionTableEntry {
    pub fn as_device_range(&self) -> BlockDeviceRange {
        BlockDeviceRange {
            start: self.first_lba,
            end: self.last_lba + 1, // last_lba is inclusive
        }
    }
}

#[derive(Debug, Clone)]
pub struct GUIDPartitionTable {
    mbr: MasterBootRecord,
    header: GPTHeader,
    partitions: Vec<GUIDPartitionTableEntry>,
}

impl GUIDPartitionTable {
    pub fn get_partitions(&self) -> &Vec<GUIDPartitionTableEntry> {
        &self.partitions
    }

    pub fn get_header(&self) -> &GPTHeader {
        &self.header
    }

    pub fn as_disk_range(&self) -> BlockDeviceRange {
        BlockDeviceRange {
            start: self.header.first_usable_lba,
            end: self.header.last_usable_lba,
        }
    }

    pub fn get_mbr(&self) -> &MasterBootRecord {
        &self.mbr
    }

    pub fn get_mbr_mut(&mut self) -> &mut MasterBootRecord {
        &mut self.mbr
    }

    pub fn read(
        block_device: Arcrwb<dyn BlockDevice>,
    ) -> Option<Either<GUIDPartitionTable, MasterBootRecord>> {
        let guard = block_device.read();
        let sector_size = guard.get_block_size();
        let max_lba = guard.get_block_count() - 1;
        drop(guard);

        let device = BlockDeviceAsCharacterDevice::new(block_device);

        let mut data = alloc::vec![0u8; 2 * sector_size as usize];
        device.read_chars(0, &mut data).ok()?;
        let mbr = unsafe { (data.as_ptr() as *const MasterBootRecord).read_unaligned() };

        if mbr.signature[0] != 0x55 || mbr.signature[1] != 0xAA {
            return None;
        }

        if mbr.partitions[0].bootable != 0
            || mbr.partitions[0].os_type != 0xEE
            || mbr.partitions[0].start_chs[0] != 0
            || mbr.partitions[0].start_chs[1] != 2
            || mbr.partitions[0].start_chs[2] != 0
            || mbr.partitions[0].start_lba != 1
            || (if max_lba > u32::MAX as u64 {
                mbr.partitions[0].sector_count != u32::MAX
            } else {
                mbr.partitions[0].sector_count != max_lba as u32
            })
        {
            return Some(Either::new_right(mbr));
        }

        for i in 1..4 {
            if !mbr.partitions[i].is_null() {
                return Some(Either::new_right(mbr));
            }
        }

        let header = unsafe {
            (data.as_ptr().add(sector_size as usize) as *const GPTHeader).read_unaligned()
        };
        drop(data);

        if header.signature != *b"EFI PART" {
            return None;
        }

        let entry_size = header.partition_entry_size as usize;
        let part_count = header.partition_entry_count as usize;

        let table_lba = header.partition_table_lba;

        let mut table = GUIDPartitionTable {
            mbr,
            header,
            partitions: Vec::with_capacity(part_count),
        };

        let mut data = alloc::vec![0u8; entry_size * part_count];
        device.read_chars(table_lba * sector_size, &mut data).ok()?;

        for i in 0..part_count {
            let offset = i * entry_size;
            let entry = unsafe {
                (data.as_ptr().add(offset) as *const GUIDPartitionTableEntryRaw).read_unaligned()
            };
            if entry.type_guid == [0; 16] {
                continue;
            }
            let name = &data[offset + 0x38..offset + entry_size];

            let partition = GUIDPartitionTableEntry {
                type_guid: entry.type_guid,
                unique_guid: entry.unique_guid,
                first_lba: entry.first_lba,
                last_lba: entry.last_lba,
                flags: entry.flags,
                name: name.iter().map(|c| *c as char).collect(),
            };

            table.partitions.push(partition);
        }

        Some(Either::new_left(table))
    }
}
