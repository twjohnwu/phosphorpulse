//! Prompt-cache countdown: the main-row `cache` segment, the per-session state
//! file under `<config dir>/cache/`, and the `cache-state` subcommand.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::render::{color, themes};
use serde_json::{json, Value};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const NOW_MS: i64 = 1_791_700_000_000;
const NOW_S: i64 = NOW_MS / 1000;
const SESSION: &str = "cache-state-session";
// matrix-tron palette.
const OK: &str = "#00FF41";
const WARN: &str = "#FF7F50";
const HOT: &str = "#FF3737";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-cache-state-{}-{}-{}",
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

fn base_command(config: &Path, home: &Path, now_ms: i64) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .env_clear()
        .env("HOME", home)
        .env("PPULSE_CONFIG_DIR", config)
        .env("PPULSE_NOW_MS", now_ms.to_string())
        .env("COLUMNS", "120")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor");
    command
}

fn run(mut command: Command, stdin: &[u8]) -> Output {
    let mut child = command
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
    child.wait_with_output().expect("wait for phosphorpulse")
}

/// Fresh config dir whose only row is the given segments, truecolor.
struct Env {
    temp: TempDir,
}

impl Env {
    fn new(segments: &[&str]) -> Self {
        let temp = TempDir::new();
        let config = temp.path().join("config");
        fs::create_dir_all(&config).expect("create config directory");
        let settings = json!({
            "activeTemplate": "matrix-tron",
            "colorDepth": "truecolor",
            "rows": [{ "layout": "fixed", "segments": segments }],
        });
        fs::write(config.join("settings.json"), settings.to_string()).expect("write settings");
        Self { temp }
    }

    fn config(&self) -> PathBuf {
        self.temp.path().join("config")
    }

    fn state_file(&self, session: &str) -> PathBuf {
        self.config().join("cache").join(format!("{session}.json"))
    }

    /// Run `render` and return the raw stdout.
    fn render_raw(&self, now_ms: i64, session: &str, prompt_cache: Option<Value>) -> String {
        let mut stdin = json!({ "session_id": session, "cwd": "/tmp/cache-state" });
        if let Some(cache) = prompt_cache {
            stdin["prompt_cache"] = cache;
        }
        let mut command = base_command(&self.config(), self.temp.path(), now_ms);
        command.arg("render");
        let output = run(command, stdin.to_string().as_bytes());
        assert!(
            output.status.success(),
            "render exited {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 stdout")
    }

    fn cache_state(&self, now_ms: i64, session: &str) -> (Option<i32>, Value) {
        let mut command = base_command(&self.config(), self.temp.path(), now_ms);
        command.args(["cache-state", "--session", session, "--json"]);
        let output = run(command, b"");
        let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "cache-state stdout must be JSON ({error}): {:?}; stderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code(), json)
    }
}

fn cache_json(ttl: &str, remaining_s: i64, warm: bool) -> Value {
    json!({
        "warm": warm,
        "caching_observed": true,
        "ttl": ttl,
        "expires_at": NOW_S + remaining_s,
        "requests": 1,
        "misses": 0,
        "expected_rebuilds": 0,
        "hit_ratio": 0.173,
        "cache_write_tokens": 68647,
        "miss_recache_tokens": 0,
        "last_miss_at": null,
        "last_miss_cause": null,
        "miss_causes": {},
        "recache_tokens_if_cold": 83025,
    })
}

/// Render the lone `cache` segment; return (ANSI-stripped trimmed text, raw).
fn cache_segment(prompt_cache: Option<Value>) -> (String, String) {
    let env = Env::new(&["cache"]);
    let raw = env.render_raw(NOW_MS, SESSION, prompt_cache);
    (strip_ansi(&raw).trim().to_owned(), raw)
}

fn assert_segment(prompt_cache: Value, text: &str, hex: &str) {
    let (stripped, raw) = cache_segment(Some(prompt_cache.clone()));
    assert_eq!(stripped, text, "text for {prompt_cache}");
    assert!(
        raw.contains(&color(hex, "truecolor", false)),
        "{text:?} must use {hex}: {raw:?}"
    );
    for other in [OK, WARN, HOT].into_iter().filter(|other| *other != hex) {
        assert!(
            !raw.contains(&color(other, "truecolor", false)),
            "{text:?} must not use {other}: {raw:?}"
        );
    }
}

/// spec 1
#[test]
fn test_cache_segment_format_and_colors() {
    // >= 10 min: whole minutes, floored; ok while more than 10% of TTL remains.
    assert_segment(cache_json("1h", 52 * 60 + 30, true), "⧗ 52m", OK);
    assert_segment(cache_json("1h", 600, true), "⧗ 10m", OK);
    // < 10 min: m:ss. 245s of a 1h TTL is under 10% (360s) -> warn.
    assert_segment(cache_json("1h", 245, true), "⧗ 4:05", WARN);
    assert_segment(cache_json("1h", 599, true), "⧗ 9:59", WARN);
    // Exactly 10% of TTL remaining is warn; just above is ok.
    assert_segment(cache_json("1h", 360, true), "⧗ 6:00", WARN);
    assert_segment(cache_json("5m", 31, true), "⧗ 0:31", OK);
    assert_segment(cache_json("5m", 30, true), "⧗ 0:30", WARN);
    // Unparseable TTL: ok while warm.
    assert_segment(cache_json("soon", 245, true), "⧗ 4:05", OK);
    // Cold: expired or warm:false; tokens use the existing token formatter.
    assert_segment(cache_json("1h", 0, true), "⧗ cold 83.0k", HOT);
    assert_segment(cache_json("1h", -30, true), "⧗ cold 83.0k", HOT);
    assert_segment(cache_json("1h", 1800, false), "⧗ cold 83.0k", HOT);
    let mut no_tokens = cache_json("1h", 0, true);
    no_tokens.as_object_mut().unwrap().remove("recache_tokens_if_cold");
    assert_segment(no_tokens, "⧗ cold", HOT);

    // Omitted without prompt_cache, or without expires_at.
    assert_eq!(cache_segment(None).0, "", "no prompt_cache");
    let mut no_expiry = cache_json("1h", 1800, true);
    no_expiry.as_object_mut().unwrap().remove("expires_at");
    assert_eq!(cache_segment(Some(no_expiry)).0, "", "no expires_at");
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copy destination");
    for entry in fs::read_dir(source).expect("read copy source") {
        let entry = entry.expect("read copy entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

/// spec 2. The golden half already passes today (stdin has no `prompt_cache`,
/// so the new segment renders nothing); only the layout assertion is RED.
#[test]
fn test_cache_default_row_and_golden_unchanged() {
    for name in themes::names() {
        let row = &themes::builtin(name).rows[0].segments;
        let at = row
            .iter()
            .position(|id| id == "cache")
            .unwrap_or_else(|| panic!("{name}: row 1 lacks cache: {row:?}"));
        assert_eq!(row.get(at + 1).map(String::as_str), Some("limitModel"), "{name}: {row:?}");
        assert_eq!(
            row.iter().map(String::as_str).collect::<Vec<_>>(),
            ["model", "effort", "git", "dir", "flex", "cache", "limitModel"],
            "{name}"
        );
    }

    let sample = Path::new("tests/golden/main-default");
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    fs::create_dir_all(&home).expect("create HOME");
    fs::create_dir_all(&config).expect("create config");
    fs::copy(sample.join("settings.json"), config.join("settings.json")).expect("copy settings");
    if sample.join("state").is_dir() {
        copy_tree(&sample.join("state"), &config);
    }
    let bin = fs::canonicalize(sample.join("bin")).expect("resolve bin");
    let env: Value =
        serde_json::from_slice(&fs::read(sample.join("env.json")).expect("read env.json"))
            .expect("parse env.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .env_clear()
        .current_dir(sample.join("cwd"))
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("PPULSE_CONFIG_DIR", &config)
        .env("PPULSE_NOW_MS", env["now_ms"].as_i64().expect("now_ms").to_string())
        .env("COLUMNS", env["COLUMNS"].as_str().expect("COLUMNS"))
        .env("TERM", env["TERM"].as_str().expect("TERM"))
        .env("COLORTERM", env["COLORTERM"].as_str().expect("COLORTERM"))
        .arg("render");
    let output = run(command, &fs::read(sample.join("stdin.json")).expect("read stdin.json"));
    assert!(output.status.success(), "golden render failed");
    let expected = fs::read(sample.join("expected.bin")).expect("read expected.bin");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&expected),
        "main-default output must stay byte-identical without prompt_cache"
    );
}

/// spec 3
#[test]
fn test_cache_state_file_written_once() {
    let env = Env::new(&["cache"]);
    let file = env.state_file(SESSION);
    assert!(!file.exists());

    env.render_raw(NOW_MS, SESSION, Some(cache_json("1h", 1800, true)));
    let first_bytes = fs::read(&file).unwrap_or_else(|error| panic!("state file {file:?}: {error}"));
    let first: Value = serde_json::from_slice(&first_bytes).expect("state file is JSON");
    assert_eq!(first["schema"], 1);
    assert_eq!(first["session_id"], SESSION);
    assert_eq!(first["warm"], true);
    assert_eq!(first["ttl"], "1h");
    assert_eq!(first["expires_at"], NOW_S + 1800);
    assert_eq!(first["recache_tokens_if_cold"], 83025);
    assert_eq!(first["updated_at"], NOW_MS);
    let first_mtime = fs::metadata(&file).unwrap().modified().unwrap();

    // Identical prompt_cache at a later now: file untouched.
    std::thread::sleep(std::time::Duration::from_millis(50));
    env.render_raw(NOW_MS + 60_000, SESSION, Some(cache_json("1h", 1800, true)));
    assert_eq!(fs::read(&file).unwrap(), first_bytes, "unchanged fields must not rewrite");
    assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), first_mtime);

    // A changed field rewrites, with a fresh updated_at.
    env.render_raw(NOW_MS + 120_000, SESSION, Some(cache_json("1h", 3600, true)));
    let third: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(third["expires_at"], NOW_S + 3600);
    assert_eq!(third["updated_at"], NOW_MS + 120_000);
}

/// spec 3, invalid session id
#[test]
fn test_cache_state_invalid_session_not_written() {
    let env = Env::new(&["cache"]);
    let outer = env.config().join("x.json");
    for bad in ["../x", "a/b", ""] {
        env.render_raw(NOW_MS, bad, Some(cache_json("1h", 1800, true)));
    }
    assert!(!outer.exists(), "path traversal must not write {outer:?}");
    let dir = env.config().join("cache");
    let entries = fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
    assert_eq!(entries, 0, "no state file for invalid ids in {dir:?}");
    // Sanity: the same stdin with a valid id does write (guards a vacuous pass).
    env.render_raw(NOW_MS, SESSION, Some(cache_json("1h", 1800, true)));
    assert!(env.state_file(SESSION).exists(), "valid id must write a file");
}

/// spec 4
#[test]
fn test_cache_state_subcommand() {
    let env = Env::new(&["cache"]);
    env.render_raw(NOW_MS, SESSION, Some(cache_json("1h", 1800, true)));

    let (code, found) = env.cache_state(NOW_MS + 100_000, SESSION);
    assert_eq!(code, Some(0));
    assert_eq!(found["schema"], 1);
    assert_eq!(found["session_id"], SESSION);
    assert_eq!(found["warm"], true);
    assert_eq!(found["ttl"], "1h");
    assert_eq!(found["expires_at"], NOW_S + 1800);
    assert_eq!(found["recache_tokens_if_cold"], 83025);
    assert_eq!(found["updated_at"], NOW_MS);
    assert_eq!(found["remaining_s"], 1700);

    // Past expiry: remaining_s goes negative, still exit 0.
    let (code, expired) = env.cache_state(NOW_MS + 1_900_000, SESSION);
    assert_eq!(code, Some(0));
    assert_eq!(expired["remaining_s"], -100);

    // Missing file and invalid id.
    for id in ["no-such-session", "../x"] {
        let (code, missing) = env.cache_state(NOW_MS, id);
        assert_eq!(code, Some(1), "{id}");
        assert_eq!(missing, json!({ "schema": 1, "error": "not-found" }), "{id}");
    }
}

/// batch 3 #11: a stored extreme `expires_at` must not panic `cache-state`.
#[test]
fn test_cache_state_extreme_expiry_no_panic() {
    let env = Env::new(&["cache"]);
    let cache_dir = env.config().join("cache");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    for (label, expires_at) in [("i64::MIN", i64::MIN), ("i64::MAX", i64::MAX), ("1e18", 1_000_000_000_000_000_000)] {
        let session = format!("extreme-{}", label.replace([':', '^'], "-"));
        let doc = json!({
            "schema": 1, "session_id": session, "warm": true, "ttl": "1h",
            "expires_at": expires_at, "recache_tokens_if_cold": null, "updated_at": NOW_MS,
        });
        fs::write(env.state_file(&session), doc.to_string()).expect("write extreme state file");
        let (code, found) = env.cache_state(NOW_MS, &session);
        assert_eq!(code, Some(0), "{label}: {found}");
        assert_eq!(found["session_id"], session, "{label}: {found}");
        // More than a day away (either direction) is no data: no remaining_s,
        // or at minimum nothing absurd.
        if let Some(remaining) = found.get("remaining_s").and_then(Value::as_i64) {
            assert!(remaining.abs() <= 86_400 * 2, "{label}: insane remaining_s {found}");
        }
    }
}

fn real_now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_millis() as i64
}

fn set_mtime(path: &Path, offset_s: i64) {
    let now = SystemTime::now();
    let when = if offset_s >= 0 {
        now + std::time::Duration::from_secs(offset_s as u64)
    } else {
        now - std::time::Duration::from_secs(offset_s.unsigned_abs())
    };
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file for mtime")
        .set_modified(when)
        .expect("set mtime");
}

fn plant_cache_files(cache_dir: &Path) -> [PathBuf; 3] {
    fs::create_dir_all(cache_dir).expect("create cache directory");
    let old = cache_dir.join("old-session.json");
    let future = cache_dir.join("future-session.json");
    let fresh = cache_dir.join("fresh-session.json");
    for path in [&old, &future, &fresh] {
        fs::write(path, "{}").expect("plant cache file");
    }
    set_mtime(&old, -10 * 86_400);
    set_mtime(&future, 3 * 86_400);
    set_mtime(&fresh, -86_400);
    [old, future, fresh]
}

/// batch 3 #12: creating a NEW session file sweeps stale and future-dated files.
#[test]
fn test_new_session_file_sweeps_stale_cache_files() {
    let env = Env::new(&["cache"]);
    let [old, future, fresh] = plant_cache_files(&env.config().join("cache"));
    let now_s = real_now_ms() / 1000;
    env.render_raw(real_now_ms(), "brand-new-session", Some({
        let mut cache = cache_json("1h", 1800, true);
        cache["expires_at"] = json!(now_s + 1800);
        cache
    }));
    assert!(env.state_file("brand-new-session").exists(), "new session file is written");
    assert!(!old.exists(), "file older than 7 days must be deleted");
    assert!(!future.exists(), "file with mtime > now + 1 day must be deleted");
    assert!(fresh.exists(), "recent file must survive");
}

/// batch 3 #12: an existing session's render must not scan the directory.
#[test]
fn test_existing_session_render_does_not_sweep() {
    let env = Env::new(&["cache"]);
    let now_ms = real_now_ms();
    let now_s = now_ms / 1000;
    let cache = |remaining: i64| {
        let mut cache = cache_json("1h", remaining, true);
        cache["expires_at"] = json!(now_s + remaining);
        cache
    };
    env.render_raw(now_ms, SESSION, Some(cache(1800)));
    assert!(env.state_file(SESSION).exists());
    let [old, future, fresh] = plant_cache_files(&env.config().join("cache"));
    // Changed field forces a rewrite of the EXISTING session file.
    env.render_raw(now_ms, SESSION, Some(cache(3000)));
    assert!(old.exists() && future.exists() && fresh.exists(), "existing session must not sweep");
}

/// batch 4 sweep regression: the sweep compares mtimes (real clock) against the
/// real clock, never `PPULSE_NOW_MS` — a far-future override must not delete a
/// one-day-old file.
#[test]
fn test_sweep_ignores_ppulse_now_ms_override() {
    const YEAR_2100_MS: i64 = 4_102_441_200_000;
    let env = Env::new(&["cache"]);
    let cache_dir = env.config().join("cache");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let other = cache_dir.join("other-session.json");
    fs::write(&other, "{}").expect("plant other session file");
    set_mtime(&other, -86_400);
    let mut cache = cache_json("1h", 1800, true);
    cache["expires_at"] = json!(YEAR_2100_MS / 1000 + 1800);
    env.render_raw(YEAR_2100_MS, "brand-new-session", Some(cache));
    assert!(env.state_file("brand-new-session").exists(), "new session file is written");
    assert!(other.exists(), "real one-day-old file must survive a year-2100 PPULSE_NOW_MS");
}
