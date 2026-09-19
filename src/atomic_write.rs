use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{Duration, SystemTime},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Replace `path` without exposing a partially-written file to readers.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let temporary = parent.join(format!("{name}.tmp-{}", std::process::id()));
    fs::write(&temporary, contents)?;
    replace_file(&temporary, path)
}

/// Replace `path` atomically after creating the temporary file with `mode`.
pub fn write_atomic_mode(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let mut temporary = parent.join(format!("{name}.tmp-{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode);
    #[cfg(not(unix))]
    let _ = mode;

    let mut file = match options.open(&temporary) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            temporary = parent.join(format!("{name}.tmp-{}-{nanos}", std::process::id()));
            options.open(&temporary)?
        }
        Err(error) => return Err(error),
    };
    file.write_all(contents)?;
    replace_file(&temporary, path)
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temporary, path)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, path: &Path) -> io::Result<()> {
    match fs::rename(temporary, path) {
        Ok(()) => Ok(()),
        // Windows' MoveFile-based rename does not replace an existing destination.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
            ) =>
        {
            fs::remove_file(path)?;
            fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

/// Best-effort hygiene for interrupted atomic writes.  Failure to inspect or
/// remove one entry must never make a state update fail.
pub fn clear_stale_temps(directory: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().contains(".tmp-") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(age) = now.duration_since(metadata.modified().unwrap_or(now)) else {
            continue;
        };
        if age > Duration::from_secs(60) {
            let _ = fs::remove_file(entry.path());
        }
    }
}
