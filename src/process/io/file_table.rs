use core::fmt::Debug;

use alloc::vec::Vec;

use crate::drivers::vfs::{Arcrwb, FileSystem};

pub const MAX_FILES: usize = 4096;

pub struct FileTable {
    pub files: Vec<Option<(Arcrwb<dyn FileSystem>, u64)>>,
    pub max_allocated_fd: usize,
    pub available_fds: Vec<usize>,
}

impl Default for FileTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FileTable {
    pub fn new() -> Self {
        Self {
            files: Vec::with_capacity(MAX_FILES),
            max_allocated_fd: 0,
            available_fds: Vec::new(),
        }
        .init()
    }

    fn init(mut self) -> Self {
        for _ in 0..MAX_FILES {
            self.files.push(None);
        }
        self
    }

    #[allow(clippy::type_complexity)]
    pub fn alloc_fd(&mut self) -> Option<(usize, &mut Option<(Arcrwb<dyn FileSystem>, u64)>)> {
        if let Some(fd) = self.available_fds.pop() {
            Some((fd, &mut self.files[fd]))
        } else if self.max_allocated_fd < MAX_FILES {
            let fd = self.max_allocated_fd;
            self.max_allocated_fd += 1;
            Some((fd, &mut self.files[fd]))
        } else {
            None
        }
    }

    pub fn free_fd(&mut self, idx: usize) -> Option<(Arcrwb<dyn FileSystem>, u64)> {
        if idx >= self.files.len() {
            return None;
        }
        self.available_fds.push(idx);
        self.files[idx].take()
    }

    pub fn get_fd(&mut self, idx: usize) -> Option<&mut Option<(Arcrwb<dyn FileSystem>, u64)>> {
        self.files.get_mut(idx)
    }
}

impl Debug for FileTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileTable").finish()
    }
}
