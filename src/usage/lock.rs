use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    time::SystemTime,
};

pub struct RefreshLock {
    path: PathBuf,
    token: String,
}

const STALE_AFTER_SECS: u64 = 30;

fn new_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", process::id(), nanos)
}

fn create(path: &Path, token: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(token.as_bytes())
}

/// Age of the existing lock file, or `None` if it can't be determined (treated as fresh).
fn lock_age(path: &Path) -> Option<std::time::Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

impl RefreshLock {
    pub fn acquire(usage_dir: &Path) -> Option<RefreshLock> {
        let path = usage_dir.join("refresh.lock");
        let token = new_token();
        match create(&path, &token) {
            Ok(()) => Some(RefreshLock { path, token }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let age = lock_age(&path);
                if age.is_none_or(|age| age.as_secs() <= STALE_AFTER_SECS) {
                    return None;
                }
                let stale_path = path.with_extension(format!("lock.stale-{token}"));
                if fs::rename(&path, &stale_path).is_err() {
                    return None;
                }
                let _ = fs::remove_file(&stale_path);
                create(&path, &token).ok()?;
                Some(RefreshLock { path, token })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(usage_dir).ok()?;
                create(&path, &token).ok()?;
                Some(RefreshLock { path, token })
            }
            Err(_) => None,
        }
    }

    pub fn release(self) {
        if fs::read_to_string(&self.path)
            .map(|contents| contents == self.token)
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
