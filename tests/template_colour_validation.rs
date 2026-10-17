//! RED coverage: template import must reject non-`#RRGGBB` colour values.

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

/// Every colour field the import schema carries, as JSON pointers.
const COLOUR_FIELDS: [&str; 9] = [
    "/segments/model/fg",
    "/segments/model/bg",
    "/rows/0/color/fg",
    "/rows/0/color/bg",
    "/subagent/colors/model/fg",
    "/subagent/colors/model/bg",
    "/gauge/okColor",
    "/gauge/warnColor",
    "/gauge/hotColor",
];

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

/// A draft carrying a valid `#RRGGBB` value in every colour field.
fn valid_draft() -> Value {
    json!({
        "rows": [{
            "layout": "auto",
            "segments": ["model", "ctx"],
            "color": {"fg": "#AABBCC", "bg": "#112233"}
        }],
        "subagent": {
            "segments": ["name"],
            "colors": {"model": {"fg": "#654321", "bg": "#123456"}}
        },
        "gauge": {
            "barWidth": 20, "warnPct": 65, "hotPct": 85,
            "okColor": "#112233", "warnColor": "#445566", "hotColor": "#778899"
        },
        "segments": {"model": {"fg": "#00FF41", "bg": "#000000"}},
        "palette": {"accent": "#00E5FF"}
    })
}

/// Save-as (via the snapshot), export, then import; returns the import result.
fn roundtrip(label: &str, draft: &Value) -> std::io::Result<Value> {
    let temp = TempDir::new(label);
    let templates_dir = temp.path().join("templates");
    let exported = temp.path().join("exported.json");
    let snapshot = draft_to_snapshot(draft);
    save_template("saved", &snapshot, &templates_dir).expect("save-as writes snapshot");
    export_template("saved", &exported, &templates_dir).expect("export saved template");
    import_template(&exported, "imported", &templates_dir)
}

fn with_value(pointer: &str, value: Value) -> Value {
    let mut draft = valid_draft();
    *draft
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("fixture has {pointer}")) = value;
    draft
}

#[test]
fn test_f00_fixture_with_valid_colours_imports() {
    // Guard: the baseline fixture itself must import, or f01/f02 prove nothing.
    roundtrip("f00", &valid_draft()).expect("all-valid fixture imports");
}

#[test]
fn test_f01_import_rejects_non_hex_colour_in_every_field() {
    let mut failures = Vec::new();
    for pointer in COLOUR_FIELDS {
        for bad in ["red", "#12345", "#GGGGGG"] {
            let label = format!("f01-{}", pointer.replace('/', "_"));
            match roundtrip(&label, &with_value(pointer, json!(bad))) {
                Ok(_) => failures.push(format!("{pointer} = {bad:?}: import accepted")),
                Err(error) if !error.to_string().contains(pointer) => {
                    failures.push(format!("{pointer} = {bad:?}: error does not name path: {error}"))
                }
                Err(_) => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "non-hex colours must be rejected with the field path:\n{}",
        failures.join("\n")
    );
}

#[test]
fn test_f02_import_accepts_valid_hex_upper_and_lower_case() {
    // Guard: may already pass.
    for pointer in COLOUR_FIELDS {
        for good in ["#AABBCC", "#aabbcc", "#0aF9bD"] {
            let label = format!("f02-{}", pointer.replace('/', "_"));
            roundtrip(&label, &with_value(pointer, json!(good)))
                .unwrap_or_else(|error| panic!("{pointer} = {good:?} rejected: {error}"));
        }
    }
}

fn assert_import_rejects(label: &str, draft: &Value, what: &str) {
    assert!(roundtrip(label, draft).is_err(), "{what} is rejected");
}

#[test]
fn test_f03_import_rejects_unknown_key_under_subagent_colors_entry() {
    // Guard: may already pass.
    let mut draft = valid_draft();
    draft["subagent"]["colors"]["model"]["underline"] = json!("#FFFFFF");
    assert_import_rejects("f03a", &draft, "unknown key under subagent.colors.<id>");
}

#[test]
fn test_f03_import_rejects_unknown_key_under_gauge() {
    // Guard: may already pass.
    let mut draft = valid_draft();
    draft["gauge"]["coolColor"] = json!("#FFFFFF");
    assert_import_rejects("f03b", &draft, "unknown key under gauge");
}

#[test]
fn test_f03_import_rejects_non_object_subagent_colors() {
    // Guard: may already pass.
    let mut draft = valid_draft();
    draft["subagent"]["colors"] = json!("#FFFFFF");
    assert_import_rejects("f03c", &draft, "non-object subagent.colors");
}

#[test]
fn test_f03_import_rejects_non_object_subagent_colors_entry() {
    // Guard: may already pass.
    let mut draft = valid_draft();
    draft["subagent"]["colors"]["model"] = json!("#FFFFFF");
    assert_import_rejects("f03d", &draft, "non-object subagent.colors.<id>");
}
