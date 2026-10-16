//! Extreme stdin timestamp values must neither hang, panic, nor produce bogus
//! countdowns. Every case drives the `render` subprocess with an isolated
//! config dir and a fixed `PPULSE_NOW_MS`.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use phosphorpulse::render::color;
use serde_json::{json, Value};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const NOW_MS: i64 = 1_791_700_000_000;
const NOW_S: i64 = NOW_MS / 1000;
const DAY_S: i64 = 86_400;
const WARN: &str = "#FF7F50";
const CONFIGURED: &str = "#123456";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-extreme-ts-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Spawn `phosphorpulse <args>`, feed `stdin`, and fail (after killing the
/// child) if it has not exited within `timeout`.
fn run_with_timeout(
    temp: &TempDir,
    config: &Path,
    args: &[&str],
    stdin: &[u8],
    timeout: Duration,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .args(args)
        .env_clear()
        .env("HOME", temp.path())
        .env("PPULSE_CONFIG_DIR", config)
        .env("PPULSE_NOW_MS", NOW_MS.to_string())
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse");
    child
        .stdin
        .take()
        .expect("open stdin")
        .write_all(stdin)
        .expect("write stdin");
    let started = Instant::now();
    loop {
        if child.try_wait().expect("poll child").is_some() {
            break;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("phosphorpulse {args:?} still running after {timeout:?}: hang");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().expect("collect output")
}

fn config_dir(temp: &TempDir, settings: Value) -> PathBuf {
    let config = temp.path().join("config");
    fs::create_dir_all(&config).expect("create config directory");
    fs::write(config.join("settings.json"), settings.to_string()).expect("write settings");
    config
}

fn simple_settings(segments: &[&str]) -> Value {
    json!({
        "activeTemplate": "matrix-tron",
        "colorDepth": "truecolor",
        "rows": [{ "layout": "fixed", "segments": segments }],
    })
}

/// Render the main row; assert exit 0 within 2s; return the raw stdout.
fn render_main(settings: Value, stdin: Value) -> String {
    let temp = TempDir::new();
    let config = config_dir(&temp, settings);
    let output = run_with_timeout(
        &temp,
        &config,
        &["render"],
        stdin.to_string().as_bytes(),
        Duration::from_secs(2),
    );
    assert!(
        output.status.success(),
        "render exited {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

fn limit_stdin(resets_at: Value) -> Value {
    json!({
        "session_id": "extreme-ts",
        "cwd": "/tmp/extreme-ts",
        "rate_limits": { "five_hour": { "used_percentage": 10, "resets_at": resets_at } },
    })
}

/// Render `limit5h` with the given `resets_at`; return ANSI-stripped text.
fn limit_text(resets_at: Value) -> String {
    strip_ansi(&render_main(simple_settings(&["limit5h"]), limit_stdin(resets_at)))
}

fn assert_gauge_without_countdown(text: &str, case: &str) {
    assert!(text.contains("10%"), "{case}: limit5h gauge must still render: {text:?}");
    assert!(!text.contains('↺'), "{case}: no countdown expected, got {text:?}");
}

fn cache_stdin(expires_at: Value, ttl: &str) -> Value {
    json!({
        "session_id": "extreme-ts-cache",
        "cwd": "/tmp/extreme-ts",
        "prompt_cache": { "warm": true, "ttl": ttl, "expires_at": expires_at },
    })
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// #8: a nine-digit year must not spin the leap-year loop.
#[test]
fn test_extreme_year_does_not_hang() {
    let text = limit_text(json!("9999999999-01-01T00:00:00Z"));
    assert_gauge_without_countdown(&text, "year 9999999999");
}

// #9: epoch seconds that overflow when scaled to ms are no data.
#[test]
fn test_string_epoch_overflow_is_no_data() {
    let text = limit_text(json!("9223372036854775807"));
    assert_gauge_without_countdown(&text, "i64::MAX string");
}

// #10: only `Z` or `±HH:MM` (HH 0-14, MM 0-59) are zones.
#[test]
fn test_iso_zone_strictness() {
    // 12:00 at +05:30 is 06:30Z; with now two hours earlier the countdown is 2h00m.
    let reset_s = days_from_civil(2026, 10, 16) * DAY_S + 6 * 3600 + 30 * 60;
    let now_s = reset_s - 2 * 3600;
    assert_eq!(now_s, now_s); // fixture arithmetic only; NOW_MS is fixed below
    let temp = TempDir::new();
    let config = config_dir(&temp, simple_settings(&["limit5h"]));
    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .arg("render")
        .env_clear()
        .env("HOME", temp.path())
        .env("PPULSE_CONFIG_DIR", &config)
        .env("PPULSE_NOW_MS", (now_s * 1000).to_string())
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("start phosphorpulse");
    child
        .stdin
        .take()
        .expect("open stdin")
        .write_all(limit_stdin(json!("2026-10-16T12:00:00+05:30")).to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success(), "render failed: {:?}", output.stderr);
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(text.contains("↺2h00m"), "+05:30 must parse as 5h30m offset: {text:?}");

    // Invalid zones are no data; collect every offender before failing.
    let mut bogus = Vec::new();
    for zone in ["+0530", "+99:00", "+05:99", "+0é:00", "+15:00"] {
        let text = limit_text(json!(format!("2026-10-16T12:00:00{zone}")));
        if text.contains('↺') || !text.contains("10%") {
            bogus.push(format!("{zone:?} -> {text:?}"));
        }
    }
    assert!(bogus.is_empty(), "invalid zones must be no data: {bogus:#?}");
}

// #11: absurd prompt_cache.expires_at values show no countdown.
#[test]
fn test_extreme_prompt_cache_expiry_shows_nothing() {
    let cases = [
        ("1e18", json!(1e18)),
        ("-1e30", json!(-1e30)),
        ("now + 2 days", json!(NOW_S + 2 * DAY_S)),
    ];
    for (label, expires_at) in cases {
        let raw = render_main(simple_settings(&["cache"]), cache_stdin(expires_at, "1h"));
        let text = strip_ansi(&raw);
        assert!(
            !text.contains('⧗'),
            "{label}: cache segment must show nothing, got {text:?}"
        );
    }
}

// #13: a configured cache fg must not mask the warn colour.
#[test]
fn test_cache_warn_beats_configured_fg() {
    let mut settings = simple_settings(&["cache"]);
    settings["segments"] = json!({ "cache": { "fg": CONFIGURED } });
    let configured = color(CONFIGURED, "truecolor", false);
    let warn = color(WARN, "truecolor", false);

    // ttl 5m, 20s left: warn.
    let raw = render_main(settings.clone(), cache_stdin(json!(NOW_S + 20), "5m"));
    assert!(strip_ansi(&raw).contains("⧗ 0:20"), "warn fixture: {raw:?}");
    assert!(raw.contains(&warn), "warn state must use {WARN}: {raw:?}");
    assert!(!raw.contains(&configured), "configured fg masks warn: {raw:?}");

    // ttl 5m, 200s left: ok state keeps the configured colour.
    let raw = render_main(settings, cache_stdin(json!(NOW_S + 200), "5m"));
    assert!(raw.contains(&configured), "ok state uses configured fg: {raw:?}");
}

// #14: a pathological subagent startTime yields no elapsed segment, no panic.
#[test]
fn test_subagent_extreme_start_time() {
    let render = |start: Value| -> String {
        let temp = TempDir::new();
        let config = temp.path().join("config");
        fs::create_dir_all(&config).expect("create config directory");
        fs::copy("tests/golden/subagent-default/settings.json", config.join("settings.json"))
            .expect("copy subagent settings");
        let stdin = json!({
            "session_id": "00000000-0000-0000-0000-000000000000",
            "columns": 120,
            "tasks": [{
                "id": "task0001",
                "type": "local_agent",
                "status": "running",
                "description": "extreme start",
                "label": "extreme start",
                "startTime": start,
                "model": "claude-sonnet-5",
                "effort": "medium",
                "contextWindowSize": 1000000,
                "tokenCount": 1000,
            }],
        });
        let output = run_with_timeout(
            &temp,
            &config,
            &["render", "--subagent"],
            stdin.to_string().as_bytes(),
            Duration::from_secs(2),
        );
        assert!(
            output.status.success(),
            "render --subagent exited {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let row: Value = serde_json::from_slice(&output.stdout).expect("stdout is one JSON row");
        strip_ansi(row["content"].as_str().expect("row has string content"))
    };
    // Control: a sane start time shows the elapsed segment (guards a vacuous pass).
    let control = render(json!(NOW_MS - 90_000));
    assert!(control.contains('⧖'), "control row must show elapsed: {control:?}");
    let row = render(json!(-1e30));
    assert!(!row.contains('⧖'), "elapsed must be empty for -1e30: {row:?}");
}

// #15: reset times beyond ~400 days are no data; a nearby one still counts down.
#[test]
fn test_far_future_reset_is_no_data() {
    let far = limit_text(json!(NOW_S + 500 * DAY_S));
    assert_gauge_without_countdown(&far, "now + 500 days");
    let near = limit_text(json!(NOW_S + 6 * DAY_S + 3600));
    assert!(near.contains("↺6d"), "now + 6d must still count down: {near:?}");
}

// B1: a cache expired up to 400 days ago is a normal cold display; only
// beyond that (or more than a day ahead) is it no data.
#[test]
fn test_long_expired_prompt_cache_renders_cold() {
    let stdin = |warm: bool, expires_at: i64| {
        let mut stdin = cache_stdin(json!(expires_at), "1h");
        stdin["prompt_cache"]["warm"] = json!(warm);
        stdin["prompt_cache"]["recache_tokens_if_cold"] = json!(120_000);
        stdin
    };
    for (label, warm, expires_at) in [
        ("1h expired, warm:false (control)", false, NOW_S - 3600),
        ("3 days expired, warm:false", false, NOW_S - 3 * DAY_S),
        ("3 days expired, warm:true", true, NOW_S - 3 * DAY_S),
        ("399 days expired, warm:true", true, NOW_S - 399 * DAY_S),
    ] {
        let raw = render_main(simple_settings(&["cache"]), stdin(warm, expires_at));
        let text = strip_ansi(&raw);
        assert!(text.contains("⧗ cold 120.0k"), "{label}: expected cold display, got {text:?}");
    }
    let raw = render_main(simple_settings(&["cache"]), stdin(true, NOW_S - 401 * DAY_S));
    assert!(!strip_ansi(&raw).contains('⧗'), "401 days expired must be no data: {raw:?}");
}

// B1: `cache-state` keeps remaining_s for a long-expired cache, same shape as 1h expired.
#[test]
fn test_cache_state_long_expired_keeps_remaining() {
    let temp = TempDir::new();
    let config = config_dir(&temp, simple_settings(&["cache"]));
    let cache_dir = config.join("cache");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let query = |session: &str, expires_at: i64| -> Value {
        let doc = json!({
            "schema": 1, "session_id": session, "warm": true, "ttl": "1h",
            "expires_at": expires_at, "recache_tokens_if_cold": null, "updated_at": NOW_MS,
        });
        fs::write(cache_dir.join(format!("{session}.json")), doc.to_string()).expect("write state");
        let output = run_with_timeout(
            &temp,
            &config,
            &["cache-state", "--session", session, "--json"],
            b"",
            Duration::from_secs(2),
        );
        assert!(output.status.success(), "cache-state failed for {session}");
        serde_json::from_slice(&output.stdout).expect("cache-state stdout is JSON")
    };
    let hour = query("expired-1h", NOW_S - 3600);
    assert_eq!(hour["remaining_s"], json!(-3600), "control: {hour}");
    let days = query("expired-3d", NOW_S - 3 * DAY_S);
    assert_eq!(days["remaining_s"], json!(-3 * DAY_S), "3-day-expired must report remaining_s: {days}");
}
