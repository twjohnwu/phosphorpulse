//! Prompt-cache countdown: the `cache` segment, its per-session state file and
//! the `cache-state` subcommand.
use super::{pomodoro::valid_session_id, simple::{tokens, Segment}};
use crate::{atomic_write, protocol::{PromptCache, RenderContext}};
use serde_json::{json, Map, Value};
use std::{fs, path::{Path, PathBuf}, time::{Duration, SystemTime}};

const SCHEMA: i64 = 1;
const MINUTE_FORMAT_FROM_S: i64 = 600;
/// A countdown more than a day in the future is a garbage `expires_at`, not data.
const MAX_REMAINING_S: i64 = 86_400;
/// An expiry more than 400 days in the past is garbage too; shorter lapses are real cold caches.
const MIN_REMAINING_S: i64 = -400 * 86_400;
const STATE_MAX_AGE: Duration = Duration::from_secs(7 * 86_400);
const STATE_MAX_FUTURE: Duration = Duration::from_secs(86_400);
const SWEEP_SCAN_LIMIT: usize = 500;

fn ttl_seconds(ttl: &str) -> Option<i64> {
    let unit = ttl.chars().last()?;
    let amount: i64 = ttl[..ttl.len() - unit.len_utf8()].parse().ok()?;
    let scale = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return None,
    };
    amount.checked_mul(scale).filter(|seconds| *seconds > 0)
}

fn remaining_s(cache: &PromptCache, now_ms: i64) -> i64 {
    // Saturating: stdin and state files can hold any i64.
    cache.expires_at.saturating_sub(now_ms.div_euclid(1000))
}

fn remaining_in_bounds(remaining: i64) -> bool {
    (MIN_REMAINING_S..=MAX_REMAINING_S).contains(&remaining)
}

/// Warn inside the last 10% of the TTL, and for any TTL longer than ten
/// minutes also once the countdown switches to `m:ss`. Unparseable TTL stays ok.
fn countdown_color(ttl: Option<&str>, remaining: i64) -> &'static str {
    match ttl.and_then(ttl_seconds) {
        Some(ttl) if remaining.saturating_mul(10) <= ttl => "warn",
        Some(ttl) if ttl > MINUTE_FORMAT_FROM_S && remaining < MINUTE_FORMAT_FROM_S => "warn",
        _ => "ok",
    }
}

pub fn segment(c: &RenderContext, now_ms: i64) -> Option<Segment> {
    let cache = c.prompt_cache.as_ref()?;
    let remaining = remaining_s(cache, now_ms);
    if !remaining_in_bounds(remaining) {
        return None;
    }
    if !cache.warm.unwrap_or(true) || remaining <= 0 {
        let text = match cache.recache_tokens_if_cold {
            Some(n) => format!("⧗ cold {}", tokens(n as f64)),
            None => "⧗ cold".into(),
        };
        return Some(Segment { text, fg: Some("hot"), bold: false });
    }
    let text = if remaining >= MINUTE_FORMAT_FROM_S {
        format!("⧗ {}m", remaining / 60)
    } else {
        format!("⧗ {}:{:02}", remaining / 60, remaining % 60)
    };
    Some(Segment {
        text,
        fg: Some(countdown_color(cache.ttl.as_deref(), remaining)),
        bold: false,
    })
}

fn state_path(config_dir: &Path, session_id: &str) -> Option<PathBuf> {
    valid_session_id(session_id).then(|| config_dir.join("cache").join(format!("{session_id}.json")))
}

fn read_state(path: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_slice(&fs::read(path).ok()?).ok()? {
        Value::Object(doc) => Some(doc),
        _ => None,
    }
}

/// Best-effort: drop state files nobody will read again (old, or future-dated by
/// a bad clock). Runs only when a new session file is created, so ordinary
/// renders never touch the directory; the scan cap bounds even that.
/// Compares file mtimes against the real wall clock, never `PPULSE_NOW_MS`: a fake
/// far-future clock in a probe would otherwise delete every live session file.
fn sweep_state_dir(cache_dir: &Path) {
    let now = SystemTime::now();
    atomic_write::clear_stale_temps(cache_dir, now);
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten().take(SWEEP_SCAN_LIMIT) {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let too_old = now.duration_since(mtime).is_ok_and(|age| age > STATE_MAX_AGE);
        let too_new = mtime.duration_since(now).is_ok_and(|ahead| ahead > STATE_MAX_FUTURE);
        if too_old || too_new {
            let _ = fs::remove_file(path);
        }
    }
}

/// Best-effort: record the latest `prompt_cache` for external readers. The file
/// is only rewritten when a field other than `updated_at` changed.
pub fn persist(c: &RenderContext, config_dir: &Path, now_ms: i64) {
    let (Some(cache), Some(session_id)) = (c.prompt_cache.as_ref(), c.session_id.as_deref()) else {
        return;
    };
    let Some(path) = state_path(config_dir, session_id) else {
        return;
    };
    let mut doc = Map::new();
    doc.insert("schema".into(), json!(SCHEMA));
    doc.insert("session_id".into(), json!(session_id));
    doc.insert("warm".into(), json!(cache.warm.unwrap_or(true)));
    doc.insert("ttl".into(), json!(cache.ttl));
    doc.insert("expires_at".into(), json!(cache.expires_at));
    doc.insert("recache_tokens_if_cold".into(), json!(cache.recache_tokens_if_cold));
    let unchanged = read_state(&path).is_some_and(|mut stored| {
        stored.remove("updated_at");
        stored == doc
    });
    if unchanged {
        return;
    }
    if !path.exists() {
        if let Some(dir) = path.parent() {
            sweep_state_dir(dir);
        }
    }
    doc.insert("updated_at".into(), json!(now_ms));
    if let Ok(bytes) = serde_json::to_vec(&Value::Object(doc)) {
        let _ = atomic_write::write_atomic(&path, &bytes);
    }
}

/// `cache-state --session <id> --json`: returns the process exit code.
pub fn state_command(args: &[String], config_dir: &Path, now_ms: i64) -> i32 {
    let session = match args {
        [session, json] if session.starts_with("--session=") && json == "--json" => {
            Some(session["--session=".len()..].to_owned())
        }
        [flag, session, json] if flag == "--session" && json == "--json" => Some(session.clone()),
        _ => None,
    };
    let stored = session
        .and_then(|id| state_path(config_dir, &id))
        .and_then(|path| read_state(&path));
    let Some(mut doc) = stored else {
        println!("{}", json!({ "schema": SCHEMA, "error": "not-found" }));
        return 1;
    };
    if let Some(expires_at) = doc.get("expires_at").and_then(Value::as_i64) {
        let remaining = expires_at.saturating_sub(now_ms.div_euclid(1000));
        // Out of bounds is no data; leave remaining_s out.
        if remaining_in_bounds(remaining) {
            doc.insert("remaining_s".into(), json!(remaining));
        }
    }
    println!("{}", Value::Object(doc));
    0
}
