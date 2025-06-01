use core::fmt::Debug;

use alloc::{boxed::Box, string::String, vec::Vec};

use crate::{
    data::file::File,
    drivers::vfs::{AsAny, OPEN_MODE_BINARY, OPEN_MODE_READ},
    formats::elf::Elf64File,
};

use super::scheduler::CreateProcessOptions;

pub trait ExecutableFileFormat: AsAny + Debug {
    fn create_process(
        &self,
        name: String,
        cmdline: String,
        cwd: String,
        uid: u32,
        gid: u32,
        supplementary_gids: Vec<u32>,
    ) -> Result<CreateProcessOptions, Box<dyn Debug>>;
}

pub fn parse_executable(path: &str) -> Result<Box<dyn ExecutableFileFormat>, Vec<Box<dyn Debug>>> {
    let mut errs: Vec<Box<dyn Debug>> = Vec::new();

    let file = match File::open(path, OPEN_MODE_BINARY | OPEN_MODE_READ) {
        Ok(file) => file,
        Err(e) => {
            errs.push(Box::new(e));
            return Err(errs);
        }
    };

    match Elf64File::try_parse(&file) {
        Ok(elf) => return Ok(Box::new(elf)),
        Err(e) => {
            errs.push(Box::new(e));
        }
    }

    match file.close() {
        Ok(..) => Err(errs),
        Err(e) => {
            errs.push(Box::new(e));
            Err(errs)
        }
    }
}
