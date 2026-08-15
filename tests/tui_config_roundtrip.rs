//! RED coverage for SaveExit persistence of phosphorpulse's own config.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{
    config::{self, model::Config},
    render,
};
use serde_json::json;

#[path = "../src/tui/app.rs"]
mod app;

use app::write_own_config;

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "phosphorpulse-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create per-test temporary directory");
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

struct EnvGuard {
    home: Option<OsString>,
    ppulse_config_dir: Option<OsString>,
}

impl EnvGuard {
    fn use_temp_home(home: &Path) -> Self {
        let guard = Self {
            home: std::env::var_os("HOME"),
            ppulse_config_dir: std::env::var_os("PPULSE_CONFIG_DIR"),
        };
        // This test has one function and never permits config::load to resolve
        // the caller's actual HOME directory.
        unsafe {
            std::env::set_var("HOME", home);
            std::env::remove_var("PPULSE_CONFIG_DIR");
        }
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.ppulse_config_dir {
                Some(value) => std::env::set_var("PPULSE_CONFIG_DIR", value),
                None => std::env::remove_var("PPULSE_CONFIG_DIR"),
            }
        }
    }
}

fn draft_fixture() -> Config {
    Config(
        json!({
            "style": "lean",
            "activeTemplate": "matrix-tron",
            "nerdFont": true,
            "leanSep": " | ",
            "colorDepth": "truecolor",
            "language": "en",
            "rows": [
                {"layout": "auto", "segments": ["model", "ctx"]},
                {"layout": "fixed", "segments": ["dir"]}
            ],
            "subagent": {"segments": ["name", "model", "ctx", "elapsed"]},
            "gauge": {"barWidth": 20, "warnPct": 65, "hotPct": 85},
            "segments": {
                "model": {"fg": "#00FF41"},
                "ctx": {"fg": "#00E5FF"},
                "dir": {"fg": "#FFFFFF", "pathDepth": 3}
            },
            "pomodoro": {"workMin": 25, "refreshSec": 5}
        })
        .as_object()
        .expect("draft fixture is an object")
        .clone(),
    )
}

/// REQ-03 / REQ-09 / S-01: SaveExit atomically writes the full own-config
/// draft, which config::load accepts and render_value can render; an unwritable
/// destination returns an error without a temporary-file residue.
#[test]
fn test_s01_draft_roundtrip_renderable() {
    let temp = TempDir::new("s01-config-roundtrip");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).expect("create temporary HOME");
    let _environment = EnvGuard::use_temp_home(&home);
    let own_config_dir = home.join(".claude/phosphorpulse");
    let draft = draft_fixture();

    write_own_config(&draft, &own_config_dir).expect("SaveExit writes own config");

    let loaded = config::load().expect("written own config loads");
    assert_eq!(loaded.path, own_config_dir.join("settings.json"));
    loaded
        .config
        .validate_renderable()
        .expect("loaded draft remains renderable");
    assert_eq!(
        loaded.config.0, draft.0,
        "round-trip preserves draft semantics"
    );
    let rendered = render::render_value(
        json!({
            "model": {"display_name": "test-model"},
            "workspace": {"current_dir": temp.path()},
            "context_window": {"used_percentage": 42}
        }),
        serde_json::Value::Object(loaded.config.0),
    );
    assert!(!rendered.is_empty(), "loaded draft renders a fixed sample");

    assert_unwritable_directory_leaves_no_partial_file(temp.path(), &draft);
}

#[cfg(unix)]
fn assert_unwritable_directory_leaves_no_partial_file(temp: &Path, draft: &Config) {
    use std::os::unix::fs::PermissionsExt;

    let locked_dir = temp.join("unwritable-own-config");
    fs::create_dir_all(&locked_dir).expect("create locked own-config directory");
    let original = fs::metadata(&locked_dir)
        .expect("stat locked directory")
        .permissions();
    let mut locked = original.clone();
    locked.set_mode(0o555);
    fs::set_permissions(&locked_dir, locked).expect("make own-config directory unwritable");

    let result = write_own_config(draft, &locked_dir);

    fs::set_permissions(&locked_dir, original).expect("restore locked directory permissions");
    assert!(
        result.is_err(),
        "unwritable own-config directory returns an error"
    );
    let residue = fs::read_dir(&locked_dir)
        .expect("read restored directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .any(|name| name.to_string_lossy().contains(".tmp-"));
    assert!(!residue, "failed atomic write leaves no temporary file");
}

#[cfg(not(unix))]
fn assert_unwritable_directory_leaves_no_partial_file(_temp: &Path, _draft: &Config) {
    panic!("S-01 requires a platform-specific unwritable-directory fixture");
}
