use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::Path,
};

/// Holds a kernel advisory lock for as long as the `File` lives; the kernel releases it on
/// process exit or crash, so a dead refresher can never wedge the lock. `_file` is `None` when
/// locking is unsupported and we failed open.
pub struct RefreshLock {
    _file: Option<File>,
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create(true).truncate(false).open(path)
}

impl RefreshLock {
    pub fn acquire(usage_dir: &Path) -> Option<RefreshLock> {
        Self::acquire_at(&usage_dir.join("refresh.lock"))
    }

    pub fn acquire_at(lock_path: &Path) -> Option<RefreshLock> {
        let file = match open(lock_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(lock_path.parent()?).ok()?;
                open(lock_path).ok()?
            }
            Err(_) => return None,
        };
        match file.try_lock() {
            Ok(()) => Some(RefreshLock { _file: Some(file) }),
            Err(TryLockError::WouldBlock) => None,
            // Filesystems without flock support: run unlocked, as the old create_new lock did.
            Err(TryLockError::Error(_)) => Some(RefreshLock { _file: None }),
        }
    }

    /// Drops the handle only. The lock file is never deleted: removing it would let one
    /// process lock the old inode while another creates and locks a new one.
    pub fn release(self) {}
}
