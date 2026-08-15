//! RED coverage for the isolated, synchronous TUI PreviewPane.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use phosphorpulse::config::model::Config;
use serde_json::json;

#[path = "../src/tui/preview.rs"]
mod preview;

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
        // This test must never let preview resolution reach the caller's HOME.
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

fn tree_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("read monitored directory") {
            let entry = entry.expect("read monitored entry");
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, snapshot);
            } else {
                snapshot.insert(
                    path.strip_prefix(root)
                        .expect("monitored entry is below root")
                        .to_path_buf(),
                    fs::read(&path).expect("read monitored file"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn preview_draft() -> Config {
    Config(
        json!({
            "activeTemplate": "matrix-tron",
            "colorDepth": "truecolor",
            "rows": [{"layout": "auto", "segments": ["model", "ctx", "pomodoro", "git", "node", "python"]}],
            "subagent": {"segments": ["name", "model", "ctx", "elapsed"]},
            "gauge": {"barWidth": 8, "warnPct": 65, "hotPct": 85},
            "pomodoro": {"workMin": 25, "refreshSec": 5}
        })
        .as_object()
        .expect("preview draft is an object")
        .clone(),
    )
}

/// REQ-04 / R4 / S-17: PreviewPane synchronously renders fixed main and
/// subagent samples with ANSI, never writes the real config directory or forks
/// live git/node/python lookups; elapsed time is reported, not asserted.
#[test]
fn test_s17_preview_isolation() {
    let temp = TempDir::new("s17-preview-isolation");
    let home = temp.path().join("home");
    let real_config = home.join(".claude/phosphorpulse");
    fs::create_dir_all(real_config.join("pomodoro")).expect("create real config canary");
    fs::write(
        real_config.join("pomodoro/shared.json"),
        b"real pomodoro canary",
    )
    .expect("write pomodoro canary");
    fs::write(
        real_config.join("lookup-cache.json"),
        b"real lookup cache canary",
    )
    .expect("write cache canary");
    let _environment = EnvGuard::use_temp_home(&home);
    let before = tree_snapshot(&real_config);

    let draft = preview_draft();
    let main_sample = json!({
        "model": {"display_name": "preview-main-model"},
        "workspace": {"current_dir": "/preview/workspace"},
        "context_window": {"used_percentage": 62},
        "session_id": "preview-session"
    });
    let subagent_sample = json!({
        "tasks": [{
            "id": "preview-subagent",
            "description": "preview-subagent sample",
            "model": "claude-preview",
            "tokenCount": 620,
            "contextWindowSize": 1000,
            "startTime": 0
        }]
    });

    let started = Instant::now();
    let first = preview::render_preview(&draft, main_sample.clone(), subagent_sample.clone());
    let second = preview::render_preview(&draft, main_sample, subagent_sample);
    println!("S-17 preview render time: {:?}", started.elapsed());

    assert_eq!(
        tree_snapshot(&real_config),
        before,
        "preview leaves the real config directory byte-for-byte unchanged"
    );
    assert!(
        first.contains("\x1b["),
        "preview includes ANSI escape styling"
    );
    assert!(
        first.contains("preview-main-model"),
        "preview includes the main sample line"
    );
    assert!(
        first.contains("preview-subagent"),
        "preview includes a subagent sample line"
    );
    assert!(
        !first.contains("git:—") && !first.contains("node:—") && !first.contains("py:—"),
        "preview API consumes fixed samples synchronously instead of live git/node/python lookups"
    );
    assert_eq!(
        first, second,
        "repeated synchronous preview renders are deterministic"
    );
}
