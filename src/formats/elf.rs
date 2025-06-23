use core::fmt::Debug;

use alloc::{boxed::Box, fmt, string::String, vec::Vec};

use crate::{
    data::{
        alloc_boxed_slice,
        file::File,
        regs::rflags::{RFlag, RFlags},
    },
    debuggable_bitset_enum,
    drivers::vfs::{SeekPosition, VfsError},
    memory::buddy_alloc::PAGE_SIZE,
    paging::{
        align_down, align_up, PageTable, DIRECT_MAPPING_OFFSET, PAGE_ACCESSED, PAGE_PRESENT,
        PAGE_RW, PAGE_USER,
    },
    process::{
        executable::ExecutableFileFormat,
        memory::PROC_USER_STACK_TOP,
        proc::{ProcessAllocatedCode, ThreadGPRegisters, ThreadState},
        scheduler::{CreateProcessOptions, ProcessSyscallABI},
    },
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfClass {
    #[default]
    None = 0,
    Class32 = 1,
    Class64 = 2,
}

impl TryFrom<u8> for ElfClass {
    type Error = ElfError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ElfClass::Class32),
            2 => Ok(ElfClass::Class64),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("bits"),
            )),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfEndianness {
    #[default]
    None = 0,
    Little = 1,
    Big = 2,
}

impl TryFrom<u8> for ElfEndianness {
    type Error = ElfError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ElfEndianness::Little),
            2 => Ok(ElfEndianness::Big),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("endianness"),
            )),
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfType {
    #[default]
    None = 0,
    Relocatable = 1,
    Executable = 2,
    Shared = 3,
    Core = 4,
}

impl TryFrom<u16> for ElfType {
    type Error = ElfError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ElfType::Relocatable),
            2 => Ok(ElfType::Executable),
            3 => Ok(ElfType::Shared),
            4 => Ok(ElfType::Core),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("elf_type"),
            )),
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfMachine {
    #[default]
    None = 0,
    X86_64 = 62,
}

impl TryFrom<u16> for ElfMachine {
    type Error = ElfError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            62 => Ok(ElfMachine::X86_64),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("instruction_set"),
            )),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfHeaderVersion {
    #[default]
    None = 0,
    Current = 1,
}

impl TryFrom<u8> for ElfHeaderVersion {
    type Error = ElfError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ElfHeaderVersion::Current),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("header_version"),
            )),
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElfVersion {
    #[default]
    None = 0,
    Current = 1,
}

impl TryFrom<u32> for ElfVersion {
    type Error = ElfError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ElfVersion::Current),
            _ => Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidField("elf_version"),
            )),
        }
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Elf64HeaderRaw {
    pub magic: [u8; 4],
    pub bits: u8,
    pub endianness: u8,
    pub header_version: u8,
    pub os_abi: u8,
    pub padding: [u8; 8],
    pub elf_type: u16,
    pub instruction_set: u16,
    pub elf_version: u32,
    pub entry_offset: u64,
    pub program_header_table_offset: u64,
    pub section_header_table_offset: u64,
    pub flags: u32,
    pub header_size: u16,
    pub program_header_entry_size: u16,
    pub program_header_entry_count: u16,
    pub section_header_entry_size: u16,
    pub section_header_entry_count: u16,
    pub index_of_section_header_string_table: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Elf64Header {
    pub magic: [u8; 4],
    pub bits: ElfClass,
    pub endianness: ElfEndianness,
    pub header_version: ElfHeaderVersion,
    pub os_abi: u8,
    pub padding: [u8; 8],
    pub elf_type: ElfType,
    pub instruction_set: ElfMachine,
    pub elf_version: ElfVersion,
    pub entry_offset: u64,
    pub program_header_table_offset: u64,
    pub section_header_table_offset: u64,
    pub flags: u32,
    pub header_size: u16,
    pub program_header_entry_size: u16,
    pub program_header_entry_count: u16,
    pub section_header_entry_size: u16,
    pub section_header_entry_count: u16,
    pub index_of_section_header_string_table: u16,
}

impl TryFrom<Elf64HeaderRaw> for Elf64Header {
    type Error = ElfError;

    fn try_from(value: Elf64HeaderRaw) -> Result<Self, Self::Error> {
        Ok(Self {
            magic: value.magic,
            bits: ElfClass::try_from(value.bits)?,
            endianness: ElfEndianness::try_from(value.endianness)?,
            header_version: ElfHeaderVersion::try_from(value.header_version)?,
            os_abi: value.os_abi,
            padding: value.padding,
            elf_type: ElfType::try_from(value.elf_type)?,
            instruction_set: ElfMachine::try_from(value.instruction_set)?,
            elf_version: ElfVersion::try_from(value.header_version as u32)?,
            entry_offset: value.entry_offset,
            program_header_table_offset: value.program_header_table_offset,
            section_header_table_offset: value.section_header_table_offset,
            flags: value.flags,
            header_size: value.header_size,
            program_header_entry_size: value.program_header_entry_size,
            program_header_entry_count: value.program_header_entry_count,
            section_header_entry_size: value.section_header_entry_size,
            section_header_entry_count: value.section_header_entry_count,
            index_of_section_header_string_table: value.index_of_section_header_string_table,
        })
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeaderRaw {
    pub segment_type: u32,
    pub flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub align: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ElfSegmentType {
    #[default]
    None,
    Load,
    Dynamic,
    Interpreter,
    Note,
    Other(u32),
}

debuggable_bitset_enum!(
    u32,
    pub enum ElfProgramHeaderFlag {
        Executable = 1,
        Writable = 2,
        Readable = 4,
    },
    ElfProgramHeaderFlags
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elf64ProgramHeader {
    pub segment_type: ElfSegmentType,
    pub flags: ElfProgramHeaderFlags,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub align: u64,
}

impl From<Elf64ProgramHeaderRaw> for Elf64ProgramHeader {
    fn from(value: Elf64ProgramHeaderRaw) -> Self {
        Self {
            segment_type: match value.segment_type {
                1 => ElfSegmentType::Load,
                2 => ElfSegmentType::Dynamic,
                3 => ElfSegmentType::Interpreter,
                4 => ElfSegmentType::Note,
                v => ElfSegmentType::Other(v),
            },
            flags: ElfProgramHeaderFlags::from(value.flags),
            p_offset: value.p_offset,
            p_vaddr: value.p_vaddr,
            p_paddr: value.p_paddr,
            p_filesz: value.p_filesz,
            p_memsz: value.p_memsz,
            align: value.align,
        }
    }
}

pub struct Elf64File {
    contents: Box<[u8]>,

    header: Elf64Header,
}

impl fmt::Debug for Elf64File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Elf64File")
            .field("header", &self.header)
            .finish()
    }
}

#[derive(Debug)]
pub enum InvalidElfFileReason {
    NoHeader,
    InvalidMagic([u8; 4]),
    InvalidField(&'static str),
}

#[derive(Debug)]
pub enum ElfError {
    InputOutput(VfsError),
    InvalidElfFile(InvalidElfFileReason),
    InvalidPageTableAllocation,
    InvalidSegmentOffset { offset: usize, filesz: usize },
}

impl From<VfsError> for ElfError {
    fn from(value: VfsError) -> Self {
        ElfError::InputOutput(value)
    }
}

pub const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

impl Elf64File {
    pub fn try_parse(file: &File) -> Result<Self, ElfError> {
        let mut buffer = [0; size_of::<Elf64HeaderRaw>()];

        file.seek(SeekPosition::FromStart(0))?;
        let size = file.read(&mut buffer)?;
        if size != size_of::<Elf64HeaderRaw>() as u64 {
            return Err(ElfError::InvalidElfFile(InvalidElfFileReason::NoHeader));
        }

        let stats = file.stats()?;

        let header_raw =
            unsafe { core::ptr::read_volatile(buffer.as_ptr() as *const Elf64HeaderRaw) };
        if header_raw.magic != ELF_MAGIC {
            return Err(ElfError::InvalidElfFile(
                InvalidElfFileReason::InvalidMagic(header_raw.magic),
            ));
        }

        let header = Elf64Header::try_from(header_raw)?;

        file.seek(SeekPosition::FromStart(0))?;

        let mut elf_file = Self {
            contents: alloc_boxed_slice(stats.size as usize),
            header,
        };

        let size = file.read(&mut elf_file.contents)?;
        if size != stats.size {
            return Err(ElfError::InputOutput(VfsError::ShortRead));
        }

        Ok(elf_file)
    }

    pub fn get_header(&self) -> &Elf64Header {
        &self.header
    }

    pub fn get_contents(&self) -> &[u8] {
        &self.contents
    }

    pub fn get_contents_ptr(&self) -> *const u8 {
        self.contents.as_ptr()
    }

    pub fn iter_program_headers<'a: 'b, 'b>(&'a self) -> Elf64ProgramHeaderIterator<'b> {
        Elf64ProgramHeaderIterator::<'b>::new(self)
    }
}

pub struct Elf64ProgramHeaderIterator<'a> {
    elf: &'a Elf64File,
    idx: usize,
}

impl<'a> Elf64ProgramHeaderIterator<'a> {
    fn new(elf: &'a Elf64File) -> Self {
        Self { elf, idx: 0 }
    }
}

impl<'a> Iterator for Elf64ProgramHeaderIterator<'a> {
    type Item = Elf64ProgramHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.elf.header.program_header_entry_count as usize {
            None
        } else {
            let ptr = self.elf.header.program_header_table_offset as usize
                + self.idx * self.elf.header.program_header_entry_size as usize;
            if ptr >= self.elf.contents.len()
                || ptr.wrapping_add(size_of::<Elf64ProgramHeaderRaw>()) > self.elf.contents.len()
                || ptr.wrapping_add(size_of::<Elf64ProgramHeaderRaw>()) <= ptr
            {
                return None;
            }
            self.idx += 1;
            unsafe {
                let addr = self.elf.contents.as_ptr().add(ptr) as *const Elf64ProgramHeaderRaw;
                let value_raw = core::ptr::read_volatile(addr);
                Some(Elf64ProgramHeader::from(value_raw))
            }
        }
    }
}

impl From<ElfError> for Box<dyn Debug> {
    fn from(value: ElfError) -> Self {
        Box::new(value)
    }
}

impl ExecutableFileFormat for Elf64File {
    fn create_process(
        &self,
        name: String,
        cmdline: String,
        cwd: String,
        uid: u32,
        gid: u32,
        supplementary_gids: Vec<u32>,
    ) -> Result<CreateProcessOptions, Box<dyn Debug>> {
        let mut pt = PageTable::alloc_new().ok_or(ElfError::InvalidPageTableAllocation)?;

        pt.map_global_higher_half();

        let mut allocated_code = Vec::new();

        for ph in self.iter_program_headers() {
            if ph.segment_type != ElfSegmentType::Load {
                continue;
            }

            let offset = ph.p_offset as usize;
            let filesz = ph.p_filesz as usize;

            let end_code = ph.p_vaddr + ph.p_filesz;

            let segment_data = self
                .contents
                .get(offset..offset + filesz)
                .ok_or(ElfError::InvalidSegmentOffset { offset, filesz })?;

            let begin_map = align_down(ph.p_vaddr, PAGE_SIZE);
            let end_map = align_up(ph.p_vaddr + ph.p_memsz, PAGE_SIZE);

            let mut code_i = 0;

            for virt in (begin_map..end_map).step_by(PAGE_SIZE as usize) {
                let mut buffer = alloc_boxed_slice(PAGE_SIZE as usize);
                if virt < ph.p_vaddr {
                    let zeros = (ph.p_vaddr - virt) as usize;
                    let rem = (PAGE_SIZE as usize - zeros).min(filesz - code_i);
                    buffer[0..zeros].fill(0);
                    if zeros + rem < PAGE_SIZE as usize {
                        buffer[zeros + rem..].fill(0);
                    }
                    buffer[zeros..zeros + rem].copy_from_slice(&segment_data[code_i..code_i + rem]);
                    code_i += rem;
                } else if virt + PAGE_SIZE >= end_code {
                    let rem = filesz - code_i;
                    buffer[0..rem].copy_from_slice(&segment_data[code_i..]);
                    code_i += rem;
                    buffer[rem..].fill(0);
                } else if code_i >= filesz {
                    buffer.fill(0);
                    code_i += PAGE_SIZE as usize;
                } else {
                    let rem = (filesz - code_i).min(PAGE_SIZE as usize);
                    buffer[0..rem].copy_from_slice(&segment_data[code_i..(code_i + rem)]);
                    code_i += rem;
                }

                let flags = PAGE_USER | PAGE_ACCESSED | PAGE_RW | PAGE_PRESENT;

                let phys = buffer.as_ptr() as u64 - DIRECT_MAPPING_OFFSET;
                unsafe {
                    pt.map_4kb(virt, phys, flags, false);
                }

                allocated_code.push((virt, buffer));
            }
        }

        Ok(CreateProcessOptions {
            name,
            cmdline,
            cwd,
            uid,
            gid,
            supplementary_gids,
            page_table: pt,
            main_thread_state: ThreadState {
                gpregs: ThreadGPRegisters {
                    rax: 0,
                    rbx: 0,
                    rcx: 0,
                    rdx: 0,
                    rsi: 0,
                    rdi: 0,
                    r8: 0,
                    r9: 0,
                    r10: 0,
                    r11: 0,
                    r12: 0,
                    r13: 0,
                    r14: 0,
                    r15: 0,
                },
                rip: self.header.entry_offset,
                rbp: PROC_USER_STACK_TOP,
                rsp: PROC_USER_STACK_TOP - 8,
                rflags: RFlags::empty()
                    .set(RFlag::InterruptFlag)
                    .set(RFlag::IOPL3)
                    .get(),
                fs_base: 0,
                gs_base: 0,
            },
            allocated_code: ProcessAllocatedCode {
                allocs: allocated_code,
            },
            syscalls: ProcessSyscallABI::Linux,
        })
    }
}
