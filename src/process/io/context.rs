use crate::process::io::file_table::FileTable;

#[derive(Debug)]
pub struct ProcessIOContext {
    pub file_table: FileTable,
}

impl Default for ProcessIOContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessIOContext {
    pub fn new() -> Self {
        Self {
            file_table: FileTable::new(),
        }
    }
}
