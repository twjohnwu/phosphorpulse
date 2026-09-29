//! RED coverage for the SaveExit hook-entry sync of pomodoro-rest-gate.

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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use phosphorpulse::{
    config::{default_config, model::Config},
    tui::{
        app::{AppState, ScreenId},
        screens::{Action, SaveExitScreen, Screen},
        settings_writer::{
            commit_claude_settings, maybe_rewrite_claude_settings, plan_claude_settings,
        },
    },
};
use serde_json::{Map, Value, json};

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
    let Ok(entries) = fs::read_dir(path.parent().expect("settings path has parent")) else {
        return Vec::new();
    };
    entries
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

fn draft(block_during_rest: bool) -> Config {
    let mut config = default_config();
    let pomodoro = config
        .0
        .entry("pomodoro")
        .or_insert_with(|| Value::Object(Map::new()));
    pomodoro
        .as_object_mut()
        .expect("pomodoro is an object")
        .insert("blockDuringRest".into(), json!(block_during_rest));
    config
}

fn own_entry() -> Value {
    json!({"type": "command", "command": "phosphorpulse pomodoro-gate", "timeout": 5})
}

fn other_group() -> Value {
    json!({"hooks": [{"type": "command", "command": "other-tool"}]})
}

fn own_group() -> Value {
    json!({"hooks": [own_entry()]})
}

/// Seed settings: statusLine/subagentStatusLine already in the merged shape so
/// only `refreshInterval` and `hooks` can legitimately change.
fn seed(refresh: u64, hooks: Option<Value>) -> String {
    let mut root = json!({
        "model": "opus",
        "statusLine": {"type": "command", "command": "phosphorpulse render", "refreshInterval": refresh},
        "subagentStatusLine": {"type": "command", "command": "phosphorpulse render --subagent"},
    });
    if let Some(hooks) = hooks {
        root["hooks"] = hooks;
    }
    serde_json::to_string_pretty(&root).unwrap()
}

fn user_prompt_hooks(groups: Vec<Value>) -> Value {
    json!({"UserPromptSubmit": groups})
}

fn seed_a() -> String {
    seed(1, None)
}
fn seed_b() -> String {
    seed(1, Some(user_prompt_hooks(vec![other_group()])))
}
fn seed_c() -> String {
    seed(1, Some(user_prompt_hooks(vec![own_group()])))
}
fn seed_d() -> String {
    seed(1, Some(user_prompt_hooks(vec![own_group(), other_group()])))
}
fn seed_g() -> String {
    seed(3, None)
}
fn seed_h() -> String {
    let slow = json!({"type": "command", "command": "phosphorpulse pomodoro-gate", "timeout": 3600});
    seed(
        1,
        Some(user_prompt_hooks(vec![
            json!({"hooks": [slow]}),
            json!({"hooks": [own_entry()]}),
        ])),
    )
}
fn seed_i() -> String {
    seed(1, Some(json!("x")))
}

fn without_hooks(value: &Value) -> Value {
    let mut copy = value.clone();
    copy.as_object_mut().expect("object").remove("hooks");
    copy
}

fn assert_standard_form(settings: &Value) {
    let groups = settings["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("UserPromptSubmit is an array");
    let own: Vec<&Value> = groups
        .iter()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .filter(|entry| {
            entry["type"] == "command" && entry["command"] == "phosphorpulse pomodoro-gate"
        })
        .collect();
    assert_eq!(own, vec![&own_entry()], "exactly one own entry in standard form");
    assert!(
        groups.contains(&own_group()),
        "own entry sits alone in a {{\"hooks\":[...]}} group"
    );
}

/// Seeds `raw` (None = no file), runs the sync, returns (result, path).
fn run(home: &Path, raw: Option<&str>, enabled: bool) -> (std::io::Result<()>, PathBuf) {
    let path = match raw {
        Some(raw) => write_settings(home, raw),
        None => settings_path(home),
    };
    (maybe_rewrite_claude_settings(&draft(enabled)), path)
}

/// Runs `f` with an isolated HOME under the env lock.
fn with_home(label: &str, f: impl FnOnce(&Path)) {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new(label);
    let _env = EnvGuard::use_temp_home(&dir.home());
    f(&dir.home());
}

/// REQ-04 / S-05
#[test]
fn test_s05_save_exit_syncs_hook_entry() {
    // true group: A, B
    with_home("s05-a", |home| {
        let original = parsed_str(&seed_a());
        let (result, path) = run(home, Some(&seed_a()), true);
        result.expect("A true");
        let after = parsed(&path);
        assert_standard_form(&after);
        assert_eq!(without_hooks(&after), without_hooks(&original), "A other keys");
        assert_eq!(backups(&path).len(), 1, "A backup count");
    });
    with_home("s05-b", |home| {
        let original = parsed_str(&seed_b());
        let (result, path) = run(home, Some(&seed_b()), true);
        result.expect("B true");
        let after = parsed(&path);
        assert_standard_form(&after);
        assert_eq!(
            after["hooks"]["UserPromptSubmit"][0],
            other_group(),
            "B other-tool group stays first"
        );
        assert_eq!(without_hooks(&after), without_hooks(&original), "B other keys");
        assert_eq!(backups(&path).len(), 1, "B backup count");
    });
    // C true: byte-unchanged, no backup
    with_home("s05-c-true", |home| {
        let (result, path) = run(home, Some(&seed_c()), true);
        result.expect("C true");
        assert_eq!(fs::read_to_string(&path).unwrap(), seed_c(), "C bytes unchanged");
        assert!(backups(&path).is_empty(), "C no backup");
    });
    // E true: creates a file with only `hooks`
    with_home("s05-e-true", |home| {
        let (result, path) = run(home, None, true);
        result.expect("E true");
        let after = parsed(&path);
        let keys: Vec<&String> = after.as_object().expect("object").keys().collect();
        assert_eq!(keys, vec!["hooks"], "E only hooks key");
        assert_standard_form(&after);
    });
    // F, I: Err, bytes unchanged, no backup
    for (label, raw) in [("f", "[]".to_string()), ("i", seed_i())] {
        with_home(&format!("s05-{label}"), |home| {
            let (result, path) = run(home, Some(&raw), true);
            assert!(result.is_err(), "{label} true must return Err");
            assert_eq!(fs::read_to_string(&path).unwrap(), raw, "{label} bytes unchanged");
            assert!(backups(&path).is_empty(), "{label} no backup");
        });
    }
    // G: one write, refreshInterval 1 and standard form, exactly one backup
    with_home("s05-g", |home| {
        let original = parsed_str(&seed_g());
        let (result, path) = run(home, Some(&seed_g()), true);
        result.expect("G true");
        let after = parsed(&path);
        assert_eq!(after["statusLine"]["refreshInterval"], json!(1));
        assert_standard_form(&after);
        let mut expected = without_hooks(&original);
        expected["statusLine"]["refreshInterval"] = json!(1);
        assert_eq!(without_hooks(&after), expected, "G other keys");
        assert_eq!(backups(&path).len(), 1, "G exactly one backup");
    });
    // H: two own entries collapse to the standard form
    with_home("s05-h", |home| {
        let original = parsed_str(&seed_h());
        let (result, path) = run(home, Some(&seed_h()), true);
        result.expect("H true");
        let after = parsed(&path);
        assert_standard_form(&after);
        assert_eq!(without_hooks(&after), without_hooks(&original), "H other keys");
    });

    // false group: C, D, E
    with_home("s05-c-false", |home| {
        let original = parsed_str(&seed_c());
        let (result, path) = run(home, Some(&seed_c()), false);
        result.expect("C false");
        let after = parsed(&path);
        assert!(after.get("hooks").is_none(), "C false removes hooks key");
        assert_eq!(without_hooks(&after), without_hooks(&original), "C false other keys");
    });
    with_home("s05-d-false", |home| {
        let original = parsed_str(&seed_d());
        let (result, path) = run(home, Some(&seed_d()), false);
        result.expect("D false");
        let after = parsed(&path);
        assert_eq!(
            after["hooks"],
            user_prompt_hooks(vec![other_group()]),
            "D false keeps only other-tool"
        );
        assert_eq!(without_hooks(&after), without_hooks(&original), "D false other keys");
    });
    with_home("s05-e-false", |home| {
        let (result, path) = run(home, None, false);
        result.expect("E false");
        assert!(!path.exists(), "E false creates nothing");
    });

    // mid-write: file rewritten between plan and commit
    with_home("s05-mid", |home| {
        let path = write_settings(home, &seed_a());
        let pending = plan_claude_settings(&draft(true))
            .expect("plan A")
            .expect("A needs a change");
        let rewritten = seed(1, Some(user_prompt_hooks(vec![other_group()])))
            .replace("opus", "sonnet");
        fs::write(&path, &rewritten).unwrap();
        assert!(commit_claude_settings(pending).is_err(), "mid-write must Err");
        assert_eq!(fs::read_to_string(&path).unwrap(), rewritten, "rewritten content kept");
    });

    // SaveExit: sync error keeps the screen open and surfaces the error
    with_home("s05-save-exit", |home| {
        let path = write_settings(home, "[]");
        let mut state = AppState::new(draft(true), ScreenId::SaveExit);
        state.config_dir = home.join("own-config");
        let action = SaveExitScreen.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(action, Action::Redraw, "sync error must not Quit");
        assert!(state.error.is_some(), "sync error must reach s.error");
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]", "F bytes unchanged");
    });
}

fn parsed_str(raw: &str) -> Value {
    serde_json::from_str(raw).expect("seed is JSON")
}
