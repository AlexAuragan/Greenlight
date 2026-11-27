//! Descriptor-relative filesystem access. Path components never follow symlinks.
//! All unsafe calls are confined here; returned descriptors own their resources.
use anyhow::{bail, Context, Result};
use std::{
    ffi::{CStr, CString, OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct Dir {
    file: File,
}
#[derive(Debug)]
pub struct Entry {
    pub name: OsString,
    pub directory: bool,
    pub regular: bool,
    pub symlink: bool,
}
pub struct Target {
    pub parent: Dir,
    pub name: OsString,
}
fn cstring(value: &OsStr) -> Result<CString> {
    Ok(CString::new(value.as_bytes())?)
}
fn owned_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: successful open/dup returns a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn parts(path: &Path) -> Result<Vec<&OsStr>> {
    path.components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            _ => bail!("unsafe relative filesystem path: {}", path.display()),
        })
        .collect()
}
impl Dir {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            bail!("anchored directory must be absolute");
        }
        let slash = c"/";
        // SAFETY: slash is a valid NUL-terminated path; flags take no mode.
        let file = owned_fd(unsafe {
            libc::open(
                slash.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?;
        let root = Self { file };
        root.open_dir(path.strip_prefix("/")?)
    }
    pub fn open_dir(&self, relative: &Path) -> Result<Self> {
        let mut current = Self {
            file: self.file.try_clone()?,
        };
        for part in parts(relative)? {
            let name = cstring(part)?;
            // SAFETY: current owns the directory FD and name is NUL-terminated.
            current = Self {
                file: owned_fd(unsafe {
                    libc::openat(
                        current.file.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                })
                .with_context(|| {
                    format!(
                        "opening directory {} without following symlinks",
                        relative.display()
                    )
                })?,
            };
        }
        Ok(current)
    }
    pub fn create_dirs(&self, relative: &Path) -> Result<Self> {
        let mut current = Self {
            file: self.file.try_clone()?,
        };
        for part in parts(relative)? {
            let name = cstring(part)?;
            // SAFETY: directory descriptor and NUL-terminated component are valid.
            let made = unsafe { libc::mkdirat(current.file.as_raw_fd(), name.as_ptr(), 0o700) };
            if made < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.into());
                }
            } else {
                current.sync()?;
            }
            current = current.open_dir(Path::new(part))?;
        }
        Ok(current)
    }
    pub fn target(&self, relative: &Path) -> Result<Target> {
        let components = parts(relative)?;
        let name = components
            .last()
            .context("file path is empty")?
            .to_os_string();
        let mut parent = PathBuf::new();
        for component in &components[..components.len() - 1] {
            parent.push(component);
        }
        Ok(Target {
            parent: self.open_dir(&parent)?,
            name,
        })
    }
    pub fn open_file(&self, relative: &Path) -> Result<File> {
        self.target(relative)?.open()
    }
    /// Inspect the leaf itself without reading data, following links, or
    /// activating a FIFO/device. O_PATH is available on supported Linux hosts.
    pub fn metadata(&self, relative: &Path) -> Result<std::fs::Metadata> {
        let target = self.target(relative)?;
        let name = cstring(&target.name)?;
        // SAFETY: parent descriptor and NUL-terminated name remain alive; the
        // returned O_PATH descriptor pins the inspected leaf, including links.
        let file = owned_fd(unsafe {
            libc::openat(
                target.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        Ok(file.metadata()?)
    }
    pub fn read(&self, relative: &Path, limit: u64) -> Result<Vec<u8>> {
        let file = self.open_file(relative)?;
        if file.metadata()?.len() > limit {
            bail!("file exceeds byte limit");
        }
        let mut data = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut data)?;
        if data.len() as u64 > limit {
            bail!("file exceeds byte limit");
        }
        Ok(data)
    }
    pub fn read_string(&self, relative: &Path, limit: u64) -> Result<String> {
        Ok(String::from_utf8(self.read(relative, limit)?)?)
    }
    pub fn create_new(&self, relative: &Path) -> Result<File> {
        self.target(relative)?.create_new()
    }
    pub fn clone_file(&self) -> Result<File> {
        Ok(self.file.try_clone()?)
    }
    pub fn lock_file(&self, relative: &Path) -> Result<File> {
        let target = self.target(relative)?;
        let name = cstring(&target.name)?;
        let file = owned_fd(unsafe {
            libc::openat(
                target.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        if !file.metadata()?.is_file() {
            bail!("lock is not a regular file");
        }
        Ok(file)
    }
    pub fn sync(&self) -> Result<()> {
        Ok(self.file.sync_all()?)
    }
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }
    pub fn entries(&self) -> Result<Entries> {
        // A new open description avoids sharing directory iteration offsets.
        let fd = owned_fd(unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?
        .into_raw_fd();
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(error.into());
        }
        Ok(Entries {
            stream,
            finished: false,
        })
    }
}
pub struct Entries {
    stream: *mut libc::DIR,
    finished: bool,
}
impl Drop for Entries {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.stream) };
    }
}
impl Iterator for Entries {
    type Item = Result<Entry>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            // SAFETY: the stream owns its FD; readdir names are copied before next call.
            unsafe { *libc::__errno_location() = 0 };
            let entry = unsafe { libc::readdir(self.stream) };
            if entry.is_null() {
                self.finished = true;
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(0) {
                    None
                } else {
                    Some(Err(error.into()))
                };
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    libc::dirfd(self.stream),
                    name.as_ptr(),
                    metadata.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } < 0
            {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::NotFound {
                    continue;
                }
                self.finished = true;
                return Some(Err(error.into()));
            }
            let kind = unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT;
            return Some(Ok(Entry {
                name: OsString::from_vec(name.to_bytes().to_vec()),
                directory: kind == libc::S_IFDIR,
                regular: kind == libc::S_IFREG,
                symlink: kind == libc::S_IFLNK,
            }));
        }
    }
}

impl Target {
    pub fn check_parent(&self, root: &Dir, relative: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let current = root.target(relative)?;
        let before = self.parent.file.metadata()?;
        let after = current.parent.file.metadata()?;
        if self.name != current.name || (before.dev(), before.ino()) != (after.dev(), after.ino()) {
            bail!("parent directory changed during operation");
        }
        Ok(())
    }
    pub fn open(&self) -> Result<File> {
        let name = cstring(&self.name)?;
        // Inspect the pinned inode before opening for reading: even a raced-in
        // FIFO/device is never opened through its file operations.
        let pinned = owned_fd(unsafe {
            libc::openat(
                self.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        if !pinned.metadata()?.is_file() {
            bail!("target is not a regular file");
        }
        // The kernel-owned descriptor link refers to that exact regular inode;
        // `pinned` stays alive until the readable descriptor is acquired.
        let file = File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd()))?;
        Ok(file)
    }
    pub fn create_new(&self) -> Result<File> {
        let name = cstring(&self.name)?;
        Ok(owned_fd(unsafe {
            libc::openat(
                self.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?)
    }
    pub fn unlink(&self) -> Result<()> {
        let name = cstring(&self.name)?;
        if unsafe { libc::unlinkat(self.parent.file.as_raw_fd(), name.as_ptr(), 0) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
        self.parent.sync()
    }
    pub fn rename_to(&self, destination: &Target) -> Result<()> {
        let from = cstring(&self.name)?;
        let to = cstring(&destination.name)?;
        if unsafe {
            libc::renameat(
                self.parent.file.as_raw_fd(),
                from.as_ptr(),
                destination.parent.file.as_raw_fd(),
                to.as_ptr(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error().into());
        }
        self.parent.sync()?;
        destination.parent.sync()
    }
    pub fn check_file(&self, expected: &File) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let expected = expected.metadata()?;
        let current = self.open()?.metadata()?;
        if (expected.dev(), expected.ino()) != (current.dev(), current.ino()) {
            bail!("published file identity changed during operation");
        }
        Ok(())
    }
    pub fn publish_new(&self, content: &[u8]) -> Result<()> {
        let temp = self.parent.target(Path::new(&format!(
            ".greenlight-export-{}.tmp",
            uuid::Uuid::new_v4()
        )))?;
        let result = (|| -> Result<()> {
            let mut file = temp.create_new()?;
            file.write_all(content)?;
            file.sync_all()?;
            let from = cstring(&temp.name)?;
            let to = cstring(&self.name)?;
            // SAFETY: both filenames and parent descriptors are valid. NOREPLACE
            // publishes atomically without replacing an existing file or link.
            if unsafe {
                libc::renameat2(
                    temp.parent.file.as_raw_fd(),
                    from.as_ptr(),
                    self.parent.file.as_raw_fd(),
                    to.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } < 0
            {
                return Err(io::Error::last_os_error().into());
            }
            self.parent.sync()?;
            self.check_file(&file)
        })();
        if result.is_err() {
            let _ = temp.unlink();
        }
        result
    }
    pub fn atomic_write(&self, content: &[u8]) -> Result<()> {
        let temp = self.parent.target(Path::new(&format!(
            ".greenlight-write-{}.tmp",
            uuid::Uuid::new_v4()
        )))?;
        let result = (|| -> Result<()> {
            let mut file = temp.create_new()?;
            match self.open() {
                Ok(previous) => file.set_permissions(previous.metadata()?.permissions())?,
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error),
            }
            file.write_all(content)?;
            file.sync_all()?;
            temp.rename_to(self)?;
            self.check_file(&file)
        })();
        if result.is_err() {
            let _ = temp.unlink();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn pinned_parent_never_follows_a_replacement_link() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/file"), "approved").unwrap();
        fs::write(outside.path().join("file"), "outside private content").unwrap();
        let dir = Dir::open(root.path()).unwrap();
        let target = dir.target(Path::new("src/file")).unwrap();
        fs::rename(root.path().join("src"), root.path().join("original")).unwrap();
        symlink(outside.path(), root.path().join("src")).unwrap();
        assert!(target.check_parent(&dir, Path::new("src/file")).is_err());
        assert!(dir.read_string(Path::new("src/file"), 100).is_err());
        // A capability already opened refers to the original authorized inode.
        target.atomic_write(b"updated original").unwrap();
        assert_eq!(
            fs::read_to_string(outside.path().join("file")).unwrap(),
            "outside private content"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("original/file")).unwrap(),
            "updated original"
        );
    }
    #[test]
    fn leaf_links_special_files_and_parent_traversal_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "private").unwrap();
        symlink(outside.path().join("secret"), root.path().join("link")).unwrap();
        let fifo = CString::new(root.path().join("fifo").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let dir = Dir::open(root.path()).unwrap();
        for path in ["link", "fifo", "../secret", "/etc/passwd"] {
            assert!(dir.read_string(Path::new(path), 100).is_err(), "{path}");
        }
        assert!(dir
            .target(Path::new("link"))
            .unwrap()
            .publish_new(b"replace")
            .is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("secret")).unwrap(),
            "private"
        );
        // Multiple enumerations must not share a directory stream offset.
        assert_eq!(
            dir.entries().unwrap().count(),
            dir.entries().unwrap().count()
        );
    }
}
