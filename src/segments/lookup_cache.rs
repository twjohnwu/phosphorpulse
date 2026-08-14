use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use crate::atomic_write::{clear_stale_temps, write_atomic};

pub const GIT_TTL_MS: i64 = 10_000;
pub const VERSION_TTL_MS: i64 = 60_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub value: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: i64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Cache {
    #[serde(default)]
    pub git: BTreeMap<String, Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<Entry>,
}

fn path(directory: &Path) -> PathBuf {
    directory.join("lookups/cache.json")
}
fn valid(entry: Option<&Entry>, ttl: i64, now: i64) -> Option<String> {
    let entry = entry?;
    (entry.fetched_at <= now && now - entry.fetched_at < ttl).then(|| entry.value.clone())
}

impl Cache {
    pub fn git(&self, cwd: &str, now: i64) -> Option<String> {
        valid(self.git.get(cwd), GIT_TTL_MS, now)
    }
    pub fn node(&self, now: i64) -> Option<String> {
        valid(self.node.as_ref(), VERSION_TTL_MS, now)
    }
    pub fn python(&self, now: i64) -> Option<String> {
        valid(self.python.as_ref(), VERSION_TTL_MS, now)
    }
}

/// Cache reads intentionally collapse all I/O, JSON, and schema failures to
/// an empty document, so rendering remains available under damaged state.
pub fn read(directory: &Path) -> Cache {
    fs::read(path(directory))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn write(directory: &Path, mut cache: Cache, now_ms: i64) {
    let lookups = directory.join("lookups");
    let now = UNIX_EPOCH + Duration::from_millis(now_ms.max(0) as u64);
    let _ = fs::create_dir_all(&lookups);
    clear_stale_temps(&lookups, now);
    cache
        .git
        .retain(|_, entry| valid(Some(entry), GIT_TTL_MS, now_ms).is_some());
    if valid(cache.node.as_ref(), VERSION_TTL_MS, now_ms).is_none() {
        cache.node = None;
    }
    if valid(cache.python.as_ref(), VERSION_TTL_MS, now_ms).is_none() {
        cache.python = None;
    }
    if let Ok(data) = serde_json::to_vec(&cache) {
        let _ = write_atomic(&path(directory), &data);
    }
}
