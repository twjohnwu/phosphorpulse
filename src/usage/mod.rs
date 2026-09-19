pub mod cache;
pub mod fetch;
pub mod lock;
pub mod trigger;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    pub display_name: String,
    pub percent: f64,
    pub resets_at: Option<i64>,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCache {
    pub fetched_at: Option<i64>,
    pub next_fetch_at: i64,
    pub limits: Vec<UsageLimit>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FetchOutcome {
    Success(Vec<UsageLimit>),
    RateLimited(Option<u64>),
    AuthFailed,
    OtherFailure,
}

pub fn parse_limits(json: &Value) -> Option<Vec<UsageLimit>> {
    let limits = json.as_object()?.get("limits")?.as_array()?;
    Some(
        limits
            .iter()
            .filter_map(|limit| {
                if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped") {
                    return None;
                }
                let name = limit
                    .get("scope")?
                    .get("model")?
                    .get("display_name")?
                    .as_str()?;
                let name = crate::protocol::clean_text(Some(name))?;
                let display_name = crate::render::row_builder::truncate_to_width(
                    &name,
                    crate::segments::simple::MAX_STATUS_WIDTH,
                );
                if display_name.is_empty() {
                    return None;
                }
                let percent = limit.get("percent")?.as_f64()?;
                if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                    return None;
                }
                Some(UsageLimit {
                    display_name,
                    percent,
                    resets_at: limit
                        .get("resets_at")
                        .and_then(|value| crate::protocol::reset(Some(value))),
                    is_active: limit
                        .get("is_active")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect(),
    )
}

pub fn schedule(
    outcome: FetchOutcome,
    old: Option<&UsageCache>,
    now: i64,
    refresh_sec: u64,
) -> UsageCache {
    match outcome {
        FetchOutcome::Success(limits) => UsageCache {
            fetched_at: Some(now),
            next_fetch_at: now.saturating_add(seconds_to_millis(refresh_sec)),
            limits,
        },
        FetchOutcome::RateLimited(retry_after) => kept_cache(
            old,
            now.saturating_add(seconds_to_millis(retry_after.unwrap_or(300).clamp(1, 3600))),
        ),
        FetchOutcome::AuthFailed => kept_cache(old, now.saturating_add(300_000)),
        FetchOutcome::OtherFailure => kept_cache(old, now.saturating_add(30_000)),
    }
}

fn kept_cache(old: Option<&UsageCache>, next_fetch_at: i64) -> UsageCache {
    UsageCache {
        fetched_at: old.and_then(|cache| cache.fetched_at),
        next_fetch_at,
        limits: old.map(|cache| cache.limits.clone()).unwrap_or_default(),
    }
}

fn seconds_to_millis(seconds: u64) -> i64 {
    i64::try_from(seconds)
        .unwrap_or(i64::MAX)
        .saturating_mul(1000)
}

pub fn read_refresh_sec(config_dir: &Path) -> u64 {
    cache::read_bounded(&config_dir.join("settings.json"), 65_536)
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
        .and_then(|settings| settings.get("usage")?.get("refreshSec")?.as_f64())
        .filter(|seconds| {
            seconds.is_finite() && seconds.fract() == 0.0 && (60.0..=600.0).contains(seconds)
        })
        .map(|seconds| seconds as u64)
        .unwrap_or(300)
}

pub fn preview_cache(now: i64) -> UsageCache {
    UsageCache {
        fetched_at: Some(now),
        next_fetch_at: now,
        limits: vec![UsageLimit {
            display_name: "Fable".to_string(),
            percent: 65.0,
            resets_at: Some(now.saturating_add(495_420_000)),
            is_active: true,
        }],
    }
}

pub fn refresh_command() -> i32 {
    let config_dir = crate::config::settings_path()
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let dir = cache::usage_dir(&config_dir);
    let Some(lock) = lock::RefreshLock::acquire(&dir) else {
        return 5;
    };

    let now = crate::clock::now_ms();
    let refresh_sec = read_refresh_sec(&config_dir);
    let cache_path = cache::cache_path(&config_dir);
    let old = cache::read_cache(&cache_path);

    let outcome = fetch::obtain_token()
        .map(|token| fetch::fetch_usage(&token))
        .unwrap_or(FetchOutcome::AuthFailed);
    let code = match outcome {
        FetchOutcome::Success(_) => 0,
        FetchOutcome::RateLimited(_) => 3,
        FetchOutcome::AuthFailed => 2,
        FetchOutcome::OtherFailure => 4,
    };

    let new_cache = schedule(outcome, old.as_ref(), now, refresh_sec);
    let _ = cache::write_cache(&cache_path, &new_cache);
    lock.release();
    code
}
