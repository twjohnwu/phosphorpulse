//! RED coverage: colour keys must survive template save-as, export, import.

#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

#[path = "../src/tui/templates_io.rs"]
mod templates_io;

use templates_io::{draft_to_snapshot, export_template, import_template, save_template};

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

fn draft_with(subagent: Value, gauge: Value) -> Value {
    json!({
        "rows": [{"layout": "auto", "segments": ["model", "ctx"]}],
        "subagent": subagent,
        "gauge": gauge,
        "segments": {"model": {"fg": "#00FF41"}},
        "palette": {"accent": "#00E5FF"}
    })
}

fn default_gauge() -> Value {
    json!({"barWidth": 20, "warnPct": 65, "hotPct": 85})
}

/// Save-as (via the snapshot), export, then import; returns the import result.
fn roundtrip(label: &str, draft: &Value) -> (Value, std::io::Result<Value>) {
    let temp = TempDir::new(label);
    let templates_dir = temp.path().join("templates");
    let exported = temp.path().join("exported.json");
    let snapshot = draft_to_snapshot(draft);
    save_template("saved", &snapshot, &templates_dir).expect("save-as writes snapshot");
    export_template("saved", &exported, &templates_dir).expect("export saved template");
    let imported = import_template(&exported, "imported", &templates_dir);
    (snapshot, imported)
}

#[test]
fn test_e01_subagent_colors_roundtrip() {
    let draft = draft_with(
        json!({"segments": ["name"], "colors": {"model": {"fg": "#654321"}}}),
        default_gauge(),
    );
    let (_, imported) = roundtrip("e01", &draft);
    let imported = imported.expect("import accepts subagent.colors");
    assert_eq!(
        imported["subagent"]["colors"]["model"]["fg"],
        json!("#654321"),
        "subagent.colors.model.fg survives save/export/import"
    );
}

#[test]
fn test_e02_gauge_colours_roundtrip() {
    let draft = draft_with(
        json!({"segments": ["name"]}),
        json!({
            "barWidth": 20, "warnPct": 65, "hotPct": 85,
            "okColor": "#112233", "warnColor": "#445566", "hotColor": "#778899"
        }),
    );
    let (_, imported) = roundtrip("e02", &draft);
    let imported = imported.expect("import accepts gauge ok/warn/hot colours");
    assert_eq!(imported["gauge"]["okColor"], json!("#112233"));
    assert_eq!(imported["gauge"]["warnColor"], json!("#445566"));
    assert_eq!(imported["gauge"]["hotColor"], json!("#778899"));
}

#[test]
fn test_e03_import_still_rejects_unknown_subagent_key() {
    let temp = TempDir::new("e03");
    let templates_dir = temp.path().join("templates");
    let source = temp.path().join("bad.json");
    let bad = draft_with(json!({"segments": ["name"], "foo": 1}), default_gauge());
    fs::write(&source, serde_json::to_vec(&bad).unwrap()).expect("write fixture");
    assert!(
        import_template(&source, "bad", &templates_dir).is_err(),
        "unknown key under subagent is rejected"
    );
}
