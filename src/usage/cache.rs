use super::UsageCache;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn usage_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("usage")
}

pub fn cache_path(config_dir: &Path) -> PathBuf {
    usage_dir(config_dir).join("cache.json")
}

pub fn lock_path(config_dir: &Path) -> PathBuf {
    usage_dir(config_dir).join("refresh.lock")
}

pub fn read_bounded(path: &Path, max: u64) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > max {
        return None;
    }
    fs::read_to_string(path).ok()
}

pub fn read_cache(path: &Path) -> Option<UsageCache> {
    serde_json::from_str(&read_bounded(path, 65_536)?).ok()
}

pub fn write_cache(path: &Path, cache: &UsageCache) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache path has no parent"))?;
    fs::create_dir_all(parent)?;
    crate::atomic_write::write_atomic(path, &serde_json::to_vec(cache)?)
}
