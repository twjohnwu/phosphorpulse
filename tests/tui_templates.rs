//! RED coverage for TUI template memento persistence.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

#[path = "../src/tui/templates_io.rs"]
mod templates_io;

use templates_io::{export_template, import_template, load_template, save_template};

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

/// Isolates HOME exactly as the TUI config round-trip test does.
struct EnvGuard {
    home: Option<OsString>,
}

impl EnvGuard {
    fn use_temp_home(home: &Path) -> Self {
        let guard = Self {
            home: std::env::var_os("HOME"),
        };
        unsafe { std::env::set_var("HOME", home) };
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
        }
    }
}

fn draft_fixture() -> Value {
    json!({
        "rows": [{"layout": "auto", "segments": ["model", "ctx"]}],
        "subagent": {"segments": ["name", "elapsed"]},
        "gauge": {"barWidth": 20, "warnPct": 65, "hotPct": 85},
        "segments": {"model": {"fg": "#00FF41"}, "dir": {"pathDepth": 3}},
        "palette": {"accent": "#00E5FF"}
    })
}

/// REQ-03 / R8 / S-04: save-as, load, export, and import preserve a whole
/// template snapshot; malformed imported JSON errors without changing draft.
#[test]
fn test_s04_memento_roundtrip() {
    let temp = TempDir::new("s04-templates");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).expect("create temporary HOME");
    let _environment = EnvGuard::use_temp_home(&home);
    let templates_dir = home.join(".claude/phosphorpulse/templates");
    let exported = temp.path().join("exported-template.json");
    let malformed = temp.path().join("malformed-template.json");
    let mut draft = draft_fixture();

    save_template("saved-draft", &draft, &templates_dir).expect("save-as writes draft");
    let loaded = load_template("saved-draft", &templates_dir).expect("load saved draft");
    assert_eq!(loaded, draft, "load round-trips the entire saved snapshot");

    export_template("saved-draft", &exported, &templates_dir).expect("export saved draft");
    let imported = import_template(&exported, "imported-draft", &templates_dir)
        .expect("import exported draft");
    assert_eq!(imported, draft, "import round-trips the exported snapshot");
    draft = imported;

    fs::write(&malformed, b"{not valid JSON").expect("write malformed import fixture");
    let before_bad_import = draft.clone();
    assert!(
        import_template(&malformed, "bad-draft", &templates_dir).is_err(),
        "malformed template import returns an error"
    );
    assert_eq!(
        draft, before_bad_import,
        "failed import leaves the draft unchanged"
    );
}
