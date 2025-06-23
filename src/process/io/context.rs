use crate::{
    data::file::File,
    drivers::{fs::virt::pipefs::Pipe, vfs::VfsError},
    process::io::file_table::FileTable,
};

#[derive(Debug)]
pub struct ProcessIOContext {
    pub stdin: File,  // fd 0
    pub stdout: File, // fd 1
    pub stderr: File, // fd 2

    pub file_table: FileTable,
}

pub struct ProcessIOContextCreateResult {
    pub stdout_read: File,
    pub stderr_read: File,
    pub io_context: ProcessIOContext,
}

impl ProcessIOContext {
    pub fn new_with_stdin(stdin_read: File) -> Result<ProcessIOContextCreateResult, VfsError> {
        let (_, stdout_read, stdout_write) = Pipe::create()?;
        let (_, stderr_read, stderr_write) = Pipe::create()?;

        Ok(ProcessIOContextCreateResult {
            stdout_read,
            stderr_read,
            io_context: Self::new_with_stdio(stdin_read, stdout_write, stderr_write),
        })
    }

    pub fn new_with_stdio(stdin_read: File, stdout_write: File, stderr_write: File) -> Self {
        let mut ft = FileTable::new();

        ft.max_allocated_fd = 3;
        unsafe {
            ft.files[0] = Some((stdin_read.get_file_system(), stdin_read.get_handle()));
            ft.files[1] = Some((stdout_write.get_file_system(), stdout_write.get_handle()));
            ft.files[2] = Some((stderr_write.get_file_system(), stderr_write.get_handle()));
        }

        Self {
            stdin: stdin_read,
            stdout: stdout_write,
            stderr: stderr_write,
            file_table: ft,
        }
    }
}
