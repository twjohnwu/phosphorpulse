//! Coverage for the 429 backoff fix and `usage.source` (local/auto/api).
//!
//! - `test_backoff_treats_low_or_absent_retry_after_as_300s` and
//!   `test_backoff_clamps_to_60_3600s` exercise `schedule` directly (no
//!   subprocess needed): a `retry-after` below 60s (including the 0 the
//!   undocumented usage endpoint returns while hard rate-limited) must not
//!   collapse the backoff to a near-instant retry loop.
//! - The remaining tests spawn the real `usage-refresh` binary, because
//!   `usage.source`/`PPULSE_CLAUDE_JSON`/`PPULSE_CONFIG_DIR` are read from
//!   process-wide env vars that can't be isolated within one test process
//!   (same reasoning as `tests/usage_refresh.rs`'s `CaseDir`).

use phosphorpulse::usage::{FetchOutcome, schedule};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const NOW: i64 = 1_789_700_000_000;
const ACTIVE_UUID: &str = "acct-00000000-0000-0000-0000-active";
const OTHER_UUID: &str = "acct-00000000-0000-0000-0000-other";

/// REQ: `schedule`'s `RateLimited` branch must treat any `retry-after` under
/// 60s as absent (falling back to 300s), not clamp it down to 1s.
///
/// RED-first evidence (fail-then-pass): before the fix, `src/usage/mod.rs`
/// did `retry_after.unwrap_or(300).clamp(1, 3600)`, so `Some(0)` produced a
/// 1s backoff instead of 300s — this test failed with
/// `assertion `left == right` failed: retry_after=Some(0)` /
/// `left: 1789700001000, right: 1789700300000` against that code, and passes
/// after the fix in `src/usage/mod.rs:90-100`.
#[test]
fn test_backoff_treats_low_or_absent_retry_after_as_300s() {
    for retry_after in [None, Some(0_u64), Some(30)] {
        let cache = schedule(FetchOutcome::RateLimited(retry_after), None, NOW, 300);
        assert_eq!(
            cache.next_fetch_at,
            NOW + 300_000,
            "retry_after={retry_after:?}"
        );
    }
}

/// REQ: a `retry-after` of 60s or above is honored (clamped to [60, 3600]).
#[test]
fn test_backoff_clamps_to_60_3600s() {
    let cache = schedule(FetchOutcome::RateLimited(Some(120)), None, NOW, 300);
    assert_eq!(cache.next_fetch_at, NOW + 120_000);

    let cache = schedule(FetchOutcome::RateLimited(Some(9999)), None, NOW, 300);
    assert_eq!(cache.next_fetch_at, NOW + 3_600_000);

    let cache = schedule(FetchOutcome::RateLimited(Some(60)), None, NOW, 300);
    assert_eq!(cache.next_fetch_at, NOW + 60_000);
}

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct CaseDir {
    path: PathBuf,
}

impl CaseDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "phosphorpulse-usage-local-{label}-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(path.join("usage")).expect("create fresh case directory");
        Self { path }
    }

    fn write_settings(&self, source: Option<&str>) {
        let usage = match source {
            Some(source) => json!({ "source": source }),
            None => json!({}),
        };
        fs::write(
            self.path.join("settings.json"),
            serde_json::to_vec(&json!({ "usage": usage })).unwrap(),
        )
        .expect("write settings");
    }

    fn write_claude_json(&self, contents: impl AsRef<[u8]>) {
        fs::write(self.path.join("claude.json"), contents).expect("write claude.json");
    }

    fn claude_json_path(&self) -> PathBuf {
        self.path.join("claude.json")
    }

    fn write_old_cache(&self, fetched_at: i64, next_fetch_at: i64) {
        fs::write(
            self.path.join("usage/cache.json"),
            serde_json::to_vec(&json!({
                "fetchedAt": fetched_at,
                "nextFetchAt": next_fetch_at,
                "limits": [{
                    "displayName": "OldModel",
                    "percent": 12.0,
                    "resetsAt": null,
                    "isActive": false
                }]
            }))
            .unwrap(),
        )
        .expect("write old cache");
    }

    fn cache(&self) -> Value {
        let bytes = fs::read(self.path.join("usage/cache.json")).expect("read cache");
        serde_json::from_slice(&bytes).expect("parse cache")
    }

    /// A token file path that is never created: proves the token isn't read
    /// on the local/auto-fresh paths, because reading it would fail and
    /// route to `AuthFailed` (exit 2) instead of a local success (exit 0).
    fn missing_token_file(&self) -> PathBuf {
        self.path.join("no-such-credentials.json")
    }

    /// Writes a valid credentials file, so a test that *does* want the API
    /// path attempted reaches the network call (and a closed port) instead
    /// of short-circuiting on `AuthFailed`.
    fn write_valid_token_file(&self) -> PathBuf {
        let path = self.path.join("credentials.json");
        fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"test-token"}}"#,
        )
        .expect("write credentials");
        path
    }

    fn run(&self) -> Output {
        self.run_with_token_file(&self.missing_token_file())
    }

    fn run_with_token_file(&self, token_file: &std::path::Path) -> Output {
        let path_env = std::env::var_os("PATH").unwrap_or_default();
        Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
            .arg("usage-refresh")
            .env_clear()
            .env("PATH", path_env)
            .env("HOME", &self.path)
            .env("PPULSE_CONFIG_DIR", &self.path)
            .env("PPULSE_NOW_MS", NOW.to_string())
            .env("PPULSE_CLAUDE_JSON", self.claude_json_path())
            .env("PPULSE_USAGE_API_URL", "http://127.0.0.1:9")
            .env("PPULSE_USAGE_TOKEN_FILE", token_file)
            .env("COLUMNS", "120")
            .output()
            .expect("run usage-refresh")
    }
}

impl Drop for CaseDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A `cachedUsageUtilization.utilization` body with two `weekly_scoped`
/// limits: one with a null `display_name` (must be skipped, exactly as
/// `parse_limits` already does for the API path) and one valid ("Fable").
fn utilization_with_one_skipped_limit() -> Value {
    json!({
        "limits": [
            {
                "kind": "weekly_scoped",
                "percent": 10,
                "resets_at": null,
                "scope": { "model": { "display_name": null } },
                "is_active": false
            },
            {
                "kind": "weekly_scoped",
                "percent": 65,
                "resets_at": "2026-09-24T05:00:00.426837+00:00",
                "scope": { "model": { "display_name": "Fable" } },
                "is_active": true
            }
        ]
    })
}

fn claude_json(account_uuid: &str, cached_uuid: &str, fetched_at_ms: i64, utilization: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "oauthAccount": { "accountUuid": account_uuid },
        "cachedUsageUtilization": {
            "fetchedAtMs": fetched_at_ms,
            "accountUuid": cached_uuid,
            "utilization": utilization
        }
    }))
    .unwrap()
}

/// REQ: `usage.source: "local"` with a fresh, matching-account cache reads
/// limits straight from `~/.claude.json`, using the file's own `fetchedAtMs`
/// (not `now`), and skips the null-`display_name` limit exactly as
/// `parse_limits` already does.
#[test]
fn test_local_source_valid_reads_limits_and_fetched_at() {
    let case = CaseDir::new("valid");
    case.write_settings(Some("local"));
    let fetched_at_ms = NOW - 3_600_000;
    case.write_claude_json(claude_json(
        ACTIVE_UUID,
        ACTIVE_UUID,
        fetched_at_ms,
        utilization_with_one_skipped_limit(),
    ));

    let output = case.run();
    assert_eq!(output.status.code(), Some(0), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(fetched_at_ms));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 300_000));
    assert_eq!(cache["limits"].as_array().map(Vec::len), Some(1));
    assert_eq!(cache["limits"][0]["displayName"], json!("Fable"));
    assert_eq!(cache["limits"][0]["percent"], json!(65.0));
    assert_eq!(cache["limits"][0]["isActive"], json!(true));
}

/// REQ: a `cachedUsageUtilization.accountUuid` that doesn't match the active
/// `oauthAccount.accountUuid` is invalid — keep the old cache, retry in 30s.
#[test]
fn test_local_source_account_uuid_mismatch_keeps_old_cache() {
    let case = CaseDir::new("mismatch");
    case.write_settings(Some("local"));
    case.write_old_cache(NOW - 600_000, NOW - 1);
    case.write_claude_json(claude_json(
        ACTIVE_UUID,
        OTHER_UUID,
        NOW - 60_000,
        utilization_with_one_skipped_limit(),
    ));

    let output = case.run();
    assert_eq!(output.status.code(), Some(4), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 30_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("OldModel"));
}

/// REQ: a missing `cachedUsageUtilization` key is invalid — keep the old
/// cache, retry in 30s.
#[test]
fn test_local_source_missing_key_keeps_old_cache() {
    let case = CaseDir::new("missing-key");
    case.write_settings(Some("local"));
    case.write_old_cache(NOW - 600_000, NOW - 1);
    case.write_claude_json(serde_json::to_vec(&json!({
        "oauthAccount": { "accountUuid": ACTIVE_UUID }
    }))
    .unwrap());

    let output = case.run();
    assert_eq!(output.status.code(), Some(4), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 30_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("OldModel"));
}

/// REQ: malformed JSON in `~/.claude.json` is invalid (after the one
/// read+parse retry) — keep the old cache, retry in 30s.
#[test]
fn test_local_source_malformed_json_keeps_old_cache() {
    let case = CaseDir::new("malformed");
    case.write_settings(Some("local"));
    case.write_old_cache(NOW - 600_000, NOW - 1);
    case.write_claude_json(b"{not json".to_vec());

    let output = case.run();
    assert_eq!(output.status.code(), Some(4), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 30_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("OldModel"));
}

/// REQ: `usage.source: "auto"` (also the default, tested here as the
/// unset/default case) with a local cache <=15min old never calls the API —
/// `PPULSE_USAGE_API_URL` points at a closed port, so any API attempt would
/// produce exit 4 with the old cache preserved instead of a local success.
/// The missing token file additionally proves the token was never read.
#[test]
fn test_auto_source_fresh_local_skips_api() {
    let case = CaseDir::new("auto-fresh");
    case.write_settings(None); // default: auto
    let fetched_at_ms = NOW - 5 * 60_000; // 5 minutes old, within the 15min window
    case.write_claude_json(claude_json(
        ACTIVE_UUID,
        ACTIVE_UUID,
        fetched_at_ms,
        utilization_with_one_skipped_limit(),
    ));

    let output = case.run();
    assert_eq!(output.status.code(), Some(0), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(fetched_at_ms));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 300_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("Fable"));
}

/// REQ: `usage.source: "auto"` with a local cache older than 15 minutes
/// falls back to the API path — with a valid token and
/// `PPULSE_USAGE_API_URL` pointing at a closed port, the network call
/// itself fails (`OtherFailure`, exit 4) with the old cache preserved,
/// proving the stale local data was NOT used and the API path was actually
/// attempted (not short-circuited on a missing token).
#[test]
fn test_auto_source_stale_local_falls_back_to_api() {
    let case = CaseDir::new("auto-stale");
    case.write_settings(Some("auto"));
    case.write_old_cache(NOW - 600_000, NOW - 1);
    let fetched_at_ms = NOW - 20 * 60_000; // 20 minutes old, past the 15min window
    case.write_claude_json(claude_json(
        ACTIVE_UUID,
        ACTIVE_UUID,
        fetched_at_ms,
        utilization_with_one_skipped_limit(),
    ));
    let token_file = case.write_valid_token_file();

    let output = case.run_with_token_file(&token_file);
    assert_eq!(output.status.code(), Some(4), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["nextFetchAt"], json!(NOW + 30_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("OldModel"));
}

/// REQ: `usage.source: "auto"` treats a `fetchedAtMs` in the future as
/// invalid, not as fresh forever — it falls back to the API path.
#[test]
fn test_auto_source_future_local_falls_back_to_api() {
    let case = CaseDir::new("auto-future");
    case.write_settings(Some("auto"));
    case.write_old_cache(NOW - 600_000, NOW - 1);
    let fetched_at_ms = NOW + 60 * 60_000; // one hour in the future
    case.write_claude_json(claude_json(
        ACTIVE_UUID,
        ACTIVE_UUID,
        fetched_at_ms,
        utilization_with_one_skipped_limit(),
    ));
    let token_file = case.write_valid_token_file();

    let output = case.run_with_token_file(&token_file);
    assert_eq!(output.status.code(), Some(4), "stderr: {:?}", output.stderr);
    let cache = case.cache();
    assert_eq!(cache["fetchedAt"], json!(NOW - 600_000));
    assert_eq!(cache["limits"][0]["displayName"], json!("OldModel"));
}
