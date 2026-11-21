//! Preserve the trusted /proc/self/fd directory prefix in SQLite's Unix VFS.
//! The default xFullPathname resolves it back to a mutable pathname.
use crate::anchored_fs::Dir;
use anyhow::{bail, Result};
use rusqlite::{ffi, Connection, OpenFlags};
use std::{ffi::CStr, sync::OnceLock};
unsafe extern "C" fn full_path(
    _: *mut ffi::sqlite3_vfs,
    input: *const libc::c_char,
    size: libc::c_int,
    output: *mut libc::c_char,
) -> libc::c_int {
    if input.is_null() || output.is_null() || size <= 0 {
        return ffi::SQLITE_CANTOPEN;
    }
    // SAFETY: SQLite provides a NUL-terminated input and output of `size` bytes.
    let path = unsafe { CStr::from_ptr(input) }.to_bytes_with_nul();
    let Some(rest) = path.strip_prefix(b"/proc/self/fd/") else {
        return ffi::SQLITE_CANTOPEN;
    };
    let Some(slash) = rest.iter().position(|byte| *byte == b'/') else {
        return ffi::SQLITE_CANTOPEN;
    };
    if slash == 0
        || !rest[..slash].iter().all(u8::is_ascii_digit)
        || &rest[slash..] != b"/history.db\0"
        || path.len() > size as usize
    {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe { std::ptr::copy(path.as_ptr().cast(), output, path.len()) };
    ffi::SQLITE_OK
}
pub fn open(directory: &Dir) -> Result<Connection> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    let registered = REGISTERED.get_or_init(|| {
        // SAFETY: SQLite's registered default VFS remains alive for the process.
        let original = unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) };
        if original.is_null() {
            return Err("SQLite default VFS unavailable".into());
        }
        let mut vfs = Box::new(unsafe { *original });
        vfs.zName = c"greenlight-anchored".as_ptr();
        vfs.pNext = std::ptr::null_mut();
        vfs.xFullPathname = Some(full_path);
        // SQLite retains this allocation until process shutdown; all other
        // callbacks and pAppData remain those of the default Unix VFS.
        let raw = Box::into_raw(vfs);
        if unsafe { ffi::sqlite3_vfs_register(raw, 0) } != ffi::SQLITE_OK {
            return Err("Cannot register anchored SQLite VFS".into());
        }
        Ok(())
    });
    if let Err(error) = registered {
        bail!("{error}");
    }
    Ok(Connection::open_with_flags_and_vfs(
        directory.path().join("history.db"),
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        "greenlight-anchored",
    )?)
}
