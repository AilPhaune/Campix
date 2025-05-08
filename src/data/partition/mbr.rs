#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MBRPartition {
    pub bootable: u8,
    pub start_chs: [u8; 3],
    pub os_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,
    pub sector_count: u32,
}

impl MBRPartition {
    pub fn is_null(&self) -> bool {
        self.bootable == 0
            && self.start_chs == [0, 0, 0]
            && self.os_type == 0
            && self.end_chs == [0, 0, 0]
            && self.start_lba == 0
            && self.sector_count == 0
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone)]
pub struct MasterBootRecord {
    pub boot_code: [u8; 446],
    pub partitions: [MBRPartition; 4],
    pub signature: [u8; 2],
}
