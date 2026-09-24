//! Reads Claude Code's own cached usage snapshot from `~/.claude.json`
//! (`cachedUsageUtilization`) so the background refresher can avoid hitting
//! the undocumented, hard-rate-limited `/api/oauth/usage` endpoint.
//!
//! Read-only: this module never writes `~/.claude.json`, and the render hot
//! path never calls it — only `refresh_command` (the background refresher).

use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{UsageLimit, cache, parse_limits};

const MAX_CLAUDE_JSON_BYTES: u64 = 8 * 1024 * 1024;
const READ_ATTEMPTS: u32 = 2;

pub struct LocalUsage {
    pub fetched_at_ms: i64,
    pub limits: Vec<UsageLimit>,
}

/// Resolves the path to Claude Code's own config file: `PPULSE_CLAUDE_JSON`
/// if set, else `$CLAUDE_CONFIG_DIR/.claude.json`, else `$HOME/.claude.json`.
pub fn claude_json_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PPULSE_CLAUDE_JSON") {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Path::new(&dir).join(".claude.json");
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(".claude.json")
}

/// Reads and parses `~/.claude.json`, retrying the read+parse once on a
/// parse failure (the file is rewritten often by Claude Code). Bounded to
/// `MAX_CLAUDE_JSON_BYTES`.
fn read_claude_json(path: &Path) -> Option<Value> {
    for _ in 0..READ_ATTEMPTS {
        if let Some(contents) = cache::read_bounded(path, MAX_CLAUDE_JSON_BYTES)
            && let Ok(value) = serde_json::from_str::<Value>(&contents)
        {
            return Some(value);
        }
    }
    None
}

/// Reads the locally cached usage snapshot. Returns `None` when the file is
/// missing/unreadable/malformed, `cachedUsageUtilization` is absent, its
/// `accountUuid` doesn't match the active `oauthAccount.accountUuid`, or the
/// `utilization` body doesn't parse via [`parse_limits`].
pub fn read_local_usage() -> Option<LocalUsage> {
    read_local_usage_at(&claude_json_path())
}

fn read_local_usage_at(path: &Path) -> Option<LocalUsage> {
    let root = read_claude_json(path)?;
    let active_uuid = root.get("oauthAccount")?.get("accountUuid")?.as_str()?;
    let cached = root.get("cachedUsageUtilization")?;
    let fetched_at_ms = cached.get("fetchedAtMs")?.as_i64()?;
    let account_uuid = cached.get("accountUuid")?.as_str()?;
    if account_uuid != active_uuid {
        return None;
    }
    let limits = parse_limits(cached.get("utilization")?)?;
    Some(LocalUsage {
        fetched_at_ms,
        limits,
    })
}
