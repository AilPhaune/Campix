use ::alloc::vec::Vec;

use crate::{
    data::{file::File, permissions::Permissions},
    debuggable_bitset_enum,
    drivers::{
        fs::virt::pipefs::Pipe,
        vfs::{
            FileStat, SeekPosition, VfsFileKind, OPEN_MODE_APPEND, OPEN_MODE_CREATE,
            OPEN_MODE_FAIL_IF_EXISTS, OPEN_MODE_READ, OPEN_MODE_WRITE,
        },
    },
    interrupts::handlers::syscall::{
        linux::{
            vfs_err_to_linux_errno, EBADF, EINVAL, EMFILE, ENOENT, ENOTDIR, EPERM, WHENCE_CUR,
            WHENCE_END, WHENCE_SET,
        },
        utils::{buffer::UserProcessBuffer, structure::UserProcessStructure},
    },
    linux_return_err_from_syscall,
    paging::PageTable,
    process::{
        memory::{get_address_space, VirtualAddressSpace},
        scheduler::ProcThreadInfo,
    },
};

const MAX_PATH_LEN: u64 = 4096;
const MAX_SINGLE_WRITE: u64 = 64 * 1024 * 1024; // 64MiB

debuggable_bitset_enum!(
    u64,
    pub enum LinuxOpenFlag {
        WriteOnly = 1 << 0,
        ReadWrite = 1 << 1,
        Create = 1 << 6,
        Excl = 1 << 7,
        Truncate = 1 << 9,
        Append = 1 << 10,
    },
    LinuxOpenFlags
);

const SUPPORTED_OPEN_FLAGS: u64 = LinuxOpenFlags::empty()
    .set(LinuxOpenFlag::WriteOnly)
    .set(LinuxOpenFlag::ReadWrite)
    .set(LinuxOpenFlag::Create)
    .set(LinuxOpenFlag::Excl)
    .set(LinuxOpenFlag::Truncate)
    .set(LinuxOpenFlag::Append)
    .get();

const SUPPORTED_PERMISSION_FLAGS: u64 = 0o7777; // sticky, setuid, setgid, rwxrwxrwx

pub enum IoAction {
    Open(LinuxOpenFlags),
    CreateChild(VfsFileKind, u64),
    Rmdir,
}

pub fn cant(_thread: &ProcThreadInfo, _stat: &FileStat, _action: IoAction) -> bool {
    // TODO: check perms
    false
}

pub fn linux_sys_read(thread: &ProcThreadInfo, fd: u64, buf: u64, count: u64) -> u64 {
    let space = get_address_space(buf);
    let Some(end_addr) = buf.checked_add(count) else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let end_space = get_address_space(end_addr);
    if !matches!(space, Some(VirtualAddressSpace::LowerHalf(..)))
        || !matches!(end_space, Some(VirtualAddressSpace::LowerHalf(..)))
    {
        linux_return_err_from_syscall!(EINVAL)
    }

    let mut ptlock = thread.thread.process.page_table.lock();
    let mut user_buffer = UserProcessBuffer::new(buf as *mut u8, count as usize);
    match user_buffer.verify_fully_mapped_mut(&mut ptlock) {
        Some(buf) => {
            let mut io_ctx = thread.thread.process.io_context.lock();
            let (fs, handle) = match io_ctx.file_table.get_fd(fd as usize) {
                Some(Some((fs, handle))) => (fs, *handle),
                _ => linux_return_err_from_syscall!(EBADF),
            };
            let mut gfs = fs.write();
            let read = match gfs.fread(handle, buf) {
                Ok(w) => w,
                Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
            };
            drop(gfs);
            drop(io_ctx);
            read
        }
        None => linux_return_err_from_syscall!(EMFILE),
    }
}

pub fn linux_sys_write(thread: &ProcThreadInfo, fd: u64, buf: u64, count: u64) -> u64 {
    if count > MAX_SINGLE_WRITE {
        linux_return_err_from_syscall!(EINVAL)
    }

    let space = get_address_space(buf);
    let Some(end_addr) = buf.checked_add(count) else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let end_space = get_address_space(end_addr);
    if !matches!(space, Some(VirtualAddressSpace::LowerHalf(..)))
        || !matches!(end_space, Some(VirtualAddressSpace::LowerHalf(..)))
    {
        linux_return_err_from_syscall!(EINVAL)
    }

    let mut ptlock = thread.thread.process.page_table.lock();
    let user_buffer = UserProcessBuffer::new(buf as *mut u8, count as usize);
    match user_buffer.verify_fully_mapped(&mut ptlock) {
        Some(buf) => {
            let mut io_ctx = thread.thread.process.io_context.lock();
            let (fs, handle) = match io_ctx.file_table.get_fd(fd as usize) {
                Some(Some((fs, handle))) => (fs, *handle),
                _ => linux_return_err_from_syscall!(EBADF),
            };
            let mut gfs = fs.write();
            let written = match gfs.fwrite(handle, buf) {
                Ok(w) => w,
                Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
            };
            drop(gfs);
            drop(io_ctx);
            written
        }
        None => {
            linux_return_err_from_syscall!(EINVAL)
        }
    }
}

pub fn linux_sys_open(thread: &ProcThreadInfo, path: u64, flags: u64, mode: u64) -> u64 {
    let mut pt = PageTable::temporary_this();

    let Some((user_buffer, true)) = UserProcessBuffer::copy_user_c_str(&mut pt, path, MAX_PATH_LEN)
    else {
        linux_return_err_from_syscall!(EINVAL)
    };

    drop(pt);

    if flags & SUPPORTED_OPEN_FLAGS != flags || mode & SUPPORTED_PERMISSION_FLAGS != mode {
        linux_return_err_from_syscall!(EINVAL)
    }

    let flags = LinuxOpenFlags::from(flags);

    let mut open_mode = 0;
    if flags.has(LinuxOpenFlag::WriteOnly) {
        open_mode |= OPEN_MODE_WRITE;
    } else {
        open_mode |= OPEN_MODE_READ;
    }
    if flags.has(LinuxOpenFlag::ReadWrite) {
        open_mode |= OPEN_MODE_WRITE;
    }
    if flags.has(LinuxOpenFlag::Create) {
        open_mode |= OPEN_MODE_CREATE;
        if flags.has(LinuxOpenFlag::Excl) {
            open_mode |= OPEN_MODE_FAIL_IF_EXISTS;
        }
    } else if flags.has(LinuxOpenFlag::Excl) {
        linux_return_err_from_syscall!(EINVAL)
    }
    if flags.has(LinuxOpenFlag::Append) {
        open_mode |= OPEN_MODE_APPEND;
    }

    let path = user_buffer
        .iter()
        .map(|x| *x as char)
        .collect::<Vec<char>>();

    let (fs, handle, _) = match File::open_raw(&path, open_mode, Permissions::from_u64(mode)) {
        Ok(f) => f,
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    };

    let stat = match File::get_stats0(&path) {
        Ok(Some(s)) => s,
        Ok(None) => linux_return_err_from_syscall!(ENOENT),
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    };

    if cant(thread, &stat, IoAction::Open(flags)) {
        linux_return_err_from_syscall!(EPERM)
    }

    if flags.has(LinuxOpenFlag::Truncate) {
        if open_mode & OPEN_MODE_WRITE != OPEN_MODE_WRITE {
            linux_return_err_from_syscall!(EINVAL)
        }
        let mut gfs = fs.write();
        if let Err(e) = gfs.fseek(handle, SeekPosition::FromStart(0)) {
            linux_return_err_from_syscall!(vfs_err_to_linux_errno(e))
        }
    }

    let mut io_ctx = thread.thread.process.io_context.lock();
    match io_ctx.file_table.alloc_fd() {
        Some((idx, f)) => {
            *f = Some((fs, handle));
            idx as u64
        }
        None => linux_return_err_from_syscall!(EMFILE),
    }
}

#[repr(C, packed(8))]
struct LinuxPipefds {
    read: u64,
    write: u64,
}

pub fn linux_sys_pipe(thread: &ProcThreadInfo, fds: u64) -> u64 {
    let mut pt = PageTable::temporary_this();

    let Some(mut structure) = UserProcessStructure::new(fds as *mut LinuxPipefds) else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let Some(fds) = structure.verify_fully_mapped_mut(&mut pt) else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let mut io_ctx = thread.thread.process.io_context.lock();
    match io_ctx.file_table.alloc_fds(2) {
        Some(alloc_fds) => {
            if alloc_fds.len() != 2 {
                linux_return_err_from_syscall!(EINVAL)
            }
            let (read, write) = (alloc_fds[0], alloc_fds[1]);

            let (_, pipe_read, pipe_write, pipe_fs) = match unsafe { Pipe::create_raw_fds() } {
                Ok(p) => p,
                Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
            };

            let Some(readfd) = io_ctx.file_table.get_fd(read) else {
                linux_return_err_from_syscall!(EINVAL)
            };
            *readfd = Some((pipe_fs.clone(), pipe_read));

            let Some(writefs) = io_ctx.file_table.get_fd(write) else {
                linux_return_err_from_syscall!(EINVAL)
            };
            *writefs = Some((pipe_fs, pipe_write));

            fds.read = read as u64;
            fds.write = write as u64;

            0
        }
        None => linux_return_err_from_syscall!(EMFILE),
    }
}

pub fn linux_sys_close(thread: &ProcThreadInfo, fd: u64) -> u64 {
    let mut io_ctx = thread.thread.process.io_context.lock();
    if let Some(Some((fs, handle))) = io_ctx.file_table.get_fd(fd as usize) {
        let mut gfs = fs.write();
        match gfs.fclose(*handle) {
            Ok(_) => {}
            Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
        }
        drop(gfs);
        0
    } else {
        linux_return_err_from_syscall!(EBADF)
    }
}

pub fn linux_sys_lseek(thread: &ProcThreadInfo, fd: u64, offset: u64, whence: u64) -> u64 {
    match whence {
        WHENCE_SET => {
            let mut io_ctx = thread.thread.process.io_context.lock();
            if let Some(Some((fs, handle))) = io_ctx.file_table.get_fd(fd as usize) {
                let mut gfs = fs.write();
                match gfs.fseek(*handle, SeekPosition::FromStart(offset)) {
                    Ok(_) => {}
                    Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
                }
                drop(gfs);
                0
            } else {
                linux_return_err_from_syscall!(EBADF)
            }
        }
        WHENCE_CUR => {
            let mut io_ctx = thread.thread.process.io_context.lock();
            if let Some(Some((fs, handle))) = io_ctx.file_table.get_fd(fd as usize) {
                let mut gfs = fs.write();
                match gfs.fseek(*handle, SeekPosition::FromCurrent(offset as i64)) {
                    Ok(_) => {}
                    Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
                }
                drop(gfs);
                0
            } else {
                linux_return_err_from_syscall!(EBADF)
            }
        }
        WHENCE_END => {
            let mut io_ctx = thread.thread.process.io_context.lock();
            if let Some(Some((fs, handle))) = io_ctx.file_table.get_fd(fd as usize) {
                let mut gfs = fs.write();
                match gfs.fseek(*handle, SeekPosition::FromEnd(offset)) {
                    Ok(_) => {}
                    Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
                }
                drop(gfs);
                0
            } else {
                linux_return_err_from_syscall!(EBADF)
            }
        }
        _ => {
            linux_return_err_from_syscall!(EINVAL)
        }
    }
}

pub fn linux_sys_mkdir(thread: &ProcThreadInfo, path: u64, mode: u64) -> u64 {
    if mode & SUPPORTED_PERMISSION_FLAGS != mode {
        linux_return_err_from_syscall!(EINVAL)
    }

    let mut pt = PageTable::temporary_this();

    let Some((user_buffer, true)) = UserProcessBuffer::copy_user_c_str(&mut pt, path, MAX_PATH_LEN)
    else {
        linux_return_err_from_syscall!(EINVAL)
    };

    drop(pt);

    let mut user_cstr = user_buffer
        .iter()
        .map(|x| *x as char)
        .collect::<Vec<char>>();
    while user_cstr.last() == Some(&'/') {
        user_cstr.pop();
    }

    let Some(last_slash) = user_cstr.iter().rposition(|x| *x == '/') else {
        linux_return_err_from_syscall!(EINVAL)
    };

    let parent_path = &user_cstr[..last_slash];

    let parent = match File::get_stats0(parent_path) {
        Ok(Some(parent)) => parent,
        Ok(None) => linux_return_err_from_syscall!(ENOENT),
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    };

    if cant(
        thread,
        &parent,
        IoAction::CreateChild(VfsFileKind::Directory, mode),
    ) {
        linux_return_err_from_syscall!(EPERM)
    }

    if !parent.is_directory {
        linux_return_err_from_syscall!(ENOTDIR)
    }

    let dir = match File::mkdir0(user_cstr) {
        Ok(dir) => dir,
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    };

    // TODO: set dir perms
    drop(dir);

    0
}

pub fn linux_sys_rmdir(thread: &ProcThreadInfo, path: u64) -> u64 {
    let mut pt = PageTable::temporary_this();

    let Some((user_buffer, true)) = UserProcessBuffer::copy_user_c_str(&mut pt, path, MAX_PATH_LEN)
    else {
        linux_return_err_from_syscall!(EINVAL)
    };

    drop(pt);

    let mut user_cstr = user_buffer
        .iter()
        .map(|x| *x as char)
        .collect::<Vec<char>>();
    while user_cstr.last() == Some(&'/') {
        user_cstr.pop();
    }

    let file = match File::get_stats0(&user_cstr) {
        Ok(Some(f)) => f,
        Ok(None) => linux_return_err_from_syscall!(ENOENT),
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    };

    if cant(thread, &file, IoAction::Rmdir) {
        linux_return_err_from_syscall!(EPERM)
    }

    if !file.is_directory {
        linux_return_err_from_syscall!(ENOTDIR)
    }

    match File::delete0(&user_cstr) {
        Ok(_) => 0,
        Err(e) => linux_return_err_from_syscall!(vfs_err_to_linux_errno(e)),
    }
}
