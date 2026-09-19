use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{CommandCache, MAX_CACHE_FILE_BYTES};

pub fn commands_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("commands")
}

pub fn cache_path(config_dir: &Path, key: &str) -> PathBuf {
    commands_dir(config_dir).join(format!("{key}.json"))
}

pub fn lock_path(config_dir: &Path, key: &str) -> PathBuf {
    commands_dir(config_dir).join(format!("{key}.lock"))
}

pub fn read_cache(path: &Path) -> Option<CommandCache> {
    serde_json::from_str(&crate::usage::cache::read_bounded(
        path,
        MAX_CACHE_FILE_BYTES,
    )?)
    .ok()
}

pub fn write_cache(path: &Path, cache: &CommandCache) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    crate::atomic_write::write_atomic_mode(path, &serde_json::to_vec(cache)?, 0o600)
}
