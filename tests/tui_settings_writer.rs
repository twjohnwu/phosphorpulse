//! RED coverage for the frozen TypeScript settings-writer parity paths.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::config::model::Config;
use serde_json::{Value, json};

#[path = "../src/tui/settings_writer.rs"]
mod settings_writer;

use settings_writer::{maybe_rewrite_claude_settings, write_statusline_blocks};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        fs::create_dir_all(&path).expect("create per-case temporary directory");
        Self(path)
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
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
        fs::create_dir_all(home).expect("create temporary HOME");
        let guard = Self {
            home: std::env::var_os("HOME"),
            ppulse_config_dir: std::env::var_os("PPULSE_CONFIG_DIR"),
        };
        // Never allow the writer under test to resolve the caller's actual HOME.
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

fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude/settings.json")
}

fn write_settings(home: &Path, raw: &str) -> PathBuf {
    let path = settings_path(home);
    fs::create_dir_all(path.parent().expect("settings path has parent"))
        .expect("create temporary .claude directory");
    fs::write(&path, raw).expect("seed temporary settings file");
    path
}

fn backups(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path.parent().expect("settings path has parent"))
        .expect("read temporary .claude directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| {
            entry
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("settings.json.bak-"))
        })
        .collect()
}

fn parsed(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read rewritten settings"))
        .expect("rewritten settings are JSON")
}

fn assert_default_blocks(settings: &Value) {
    assert_eq!(
        settings["statusLine"],
        json!({"type": "command", "command": "phosphorpulse render", "refreshInterval": 1}),
    );
    assert_eq!(
        settings["subagentStatusLine"],
        json!({"type": "command", "command": "phosphorpulse render --subagent"}),
    );
}

fn draft(rows: Value, pomodoro: Option<Value>) -> Config {
    let mut root = serde_json::Map::new();
    root.insert("rows".into(), rows);
    if let Some(pomodoro) = pomodoro {
        root.insert("pomodoro".into(), pomodoro);
    }
    Config(root)
}

/// REQ-03 / REQ-09 / S-02: the Wizard/SettingsInstall writer creates an
/// absent settings file without a backup, but always backs up and rewrites any
/// existing file, including one whose blocks are already correct.
#[test]
fn test_s02_statusline_blocks_write() {
    let _env_lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let absent = TempDir::new("s02-absent");
    let absent_home = absent.home();
    let _environment = EnvGuard::use_temp_home(&absent_home);
    write_statusline_blocks(None).expect("absent settings file is created");
    let absent_path = settings_path(&absent_home);
    assert_default_blocks(&parsed(&absent_path));
    assert!(
        backups(&absent_path).is_empty(),
        "absent-file branch has no backup"
    );

    let missing = TempDir::new("s02-missing-blocks");
    let missing_home = missing.home();
    let missing_path = write_settings(&missing_home, "{\n    \"unrelated\": {\"keep\": true}\n}\n");
    let _environment = EnvGuard::use_temp_home(&missing_home);
    write_statusline_blocks(None).expect("missing blocks are inserted");
    let missing_written = parsed(&missing_path);
    assert_default_blocks(&missing_written);
    assert_eq!(missing_written["unrelated"], json!({"keep": true}));
    let missing_backups = backups(&missing_path);
    assert_eq!(missing_backups.len(), 1, "existing file is backed up first");
    assert_eq!(
        fs::read_to_string(&missing_backups[0]).expect("read backup"),
        "{\n    \"unrelated\": {\"keep\": true}\n}\n"
    );

    let old = TempDir::new("s02-old-blocks");
    let old_home = old.home();
    let old_raw = "{\"unrelated\":7,\"statusLine\":{\"type\":\"command\",\"command\":\"custom\"},\"subagentStatusLine\":{\"command\":\"old\"}}";
    let old_path = write_settings(&old_home, old_raw);
    let _environment = EnvGuard::use_temp_home(&old_home);
    write_statusline_blocks(None).expect("old blocks are replaced");
    let old_written = parsed(&old_path);
    assert_default_blocks(&old_written);
    assert_eq!(old_written["unrelated"], json!(7));
    let old_backups = backups(&old_path);
    assert_eq!(old_backups.len(), 1);
    assert_eq!(
        fs::read_to_string(&old_backups[0]).expect("read backup"),
        old_raw
    );

    let correct = TempDir::new("s02-correct-blocks");
    let correct_home = correct.home();
    let correct_raw = "{\"subagentStatusLine\":{\"type\":\"command\",\"command\":\"phosphorpulse render --subagent\"},\"statusLine\":{\"type\":\"command\",\"command\":\"phosphorpulse render\"},\"unrelated\":[1,2]}";
    let correct_path = write_settings(&correct_home, correct_raw);
    let _environment = EnvGuard::use_temp_home(&correct_home);
    write_statusline_blocks(None).expect("already-correct blocks still rewrite");
    assert_default_blocks(&parsed(&correct_path));
    assert_eq!(
        backups(&correct_path).len(),
        1,
        "no no-op path for existing files"
    );
    assert_eq!(
        fs::read_to_string(&correct_path).expect("read normalized write"),
        serde_json::to_string_pretty(&parsed(&correct_path)).expect("serialize expected JSON")
    );
}

/// REQ-03 / REQ-09 / S-03: SaveExit conditionally rebuilds both blocks using
/// DEFAULT_COMMANDS (overwriting custom commands), always sets
/// refreshInterval to 1, and leaves bytes alone when it is already 1.
#[test]
fn test_s03_saveexit_rewrite_semantics() {
    let _env_lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let added = TempDir::new("s03-refresh-added");
    let added_home = added.home();
    let added_path = write_settings(
        &added_home,
        "{\"unrelated\":true,\"statusLine\":{\"type\":\"command\",\"command\":\"custom\"}}",
    );
    let _environment = EnvGuard::use_temp_home(&added_home);
    maybe_rewrite_claude_settings(&draft(json!([{"segments": ["model"]}]), None))
        .expect("adding refresh interval rewrites settings");
    let added_written = parsed(&added_path);
    assert_eq!(
        added_written["statusLine"],
        json!({"type": "command", "command": "phosphorpulse render", "refreshInterval": 1})
    );
    assert_eq!(
        added_written["subagentStatusLine"],
        json!({"type": "command", "command": "phosphorpulse render --subagent"})
    );
    assert_eq!(added_written["unrelated"], json!(true));
    assert_eq!(backups(&added_path).len(), 1);

    let changed = TempDir::new("s03-refresh-normalized");
    let changed_home = changed.home();
    let changed_path = write_settings(
        &changed_home,
        "{\"statusLine\":{\"type\":\"command\",\"command\":\"custom\",\"refreshInterval\":3},\"unrelated\":\"keep\"}",
    );
    let _environment = EnvGuard::use_temp_home(&changed_home);
    maybe_rewrite_claude_settings(&draft(
        json!([{"segments": ["pomodoro"]}]),
        Some(json!({"refreshSec": 9})),
    ))
    .expect("changed refresh interval rewrites settings");
    assert_eq!(
        parsed(&changed_path)["statusLine"],
        json!({"type": "command", "command": "phosphorpulse render", "refreshInterval": 1})
    );
    assert_eq!(backups(&changed_path).len(), 1);

    let removed = TempDir::new("s03-non-pomodoro");
    let removed_home = removed.home();
    let removed_path = write_settings(
        &removed_home,
        "{\"statusLine\":{\"type\":\"command\",\"command\":\"custom\",\"refreshInterval\":3},\"unrelated\":42}",
    );
    let _environment = EnvGuard::use_temp_home(&removed_home);
    maybe_rewrite_claude_settings(&draft(json!([{"segments": ["model"]}]), None))
        .expect("non-pomodoro draft rewrites settings");
    let removed_written = parsed(&removed_path);
    assert_eq!(
        removed_written["statusLine"],
        json!({"type": "command", "command": "phosphorpulse render", "refreshInterval": 1})
    );
    assert_eq!(
        removed_written["subagentStatusLine"],
        json!({"type": "command", "command": "phosphorpulse render --subagent"})
    );
    assert_eq!(removed_written["unrelated"], json!(42));
    assert_eq!(backups(&removed_path).len(), 1);

    let unchanged = TempDir::new("s03-unchanged");
    let unchanged_home = unchanged.home();
    let unchanged_raw =
        "{\n  \"statusLine\": {\"refreshInterval\": 1},\n  \"unrelated\": [3, 2, 1]\n}\n";
    let unchanged_path = write_settings(&unchanged_home, unchanged_raw);
    let _environment = EnvGuard::use_temp_home(&unchanged_home);
    maybe_rewrite_claude_settings(&draft(
        json!([{"segments": ["pomodoro"]}]),
        Some(json!({"refreshSec": 7})),
    ))
    .expect("unchanged SaveExit condition succeeds without writing");
    assert_eq!(
        fs::read_to_string(&unchanged_path).expect("read untouched settings"),
        unchanged_raw
    );
    assert!(
        backups(&unchanged_path).is_empty(),
        "no trigger means no backup or rewrite"
    );
}
