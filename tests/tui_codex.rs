//! RED coverage for the Codex Settings config editor and simulated preview.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "../src/tui/codex/config_io.rs"]
mod config_io;
#[path = "../src/tui/codex/preview.rs"]
mod preview;

use config_io::{CodexConfig, ConfigIoError};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("create test temporary directory");
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

fn config_fixture() -> &'static str {
    "# preserve this header exactly\n[other]\nkeep = \"this formatting\" # untouched\n\n[tui]\n# the editor owns only these three keys\nstatus_line = [\n  \"model-with-reasoning\", # known\n  \"plugin-keeps-its-place\", # unknown\n  \"context-used\",\n  \"weekly-limit\",\n]\nstatus_line_use_colors = true\ntheme = \"Matrix-Tron\"\n\n[future]\nvalue = 7\n"
}

fn fused_tmtheme_fixture() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#D0D0D0</string></dict></dict>
<dict><key>name</key><string>Fused usage and limits</string><key>scope</key><string>constant.numeric, constant.language</string><key>settings</key><dict><key>foreground</key><string>#11AA22</string></dict></dict>
</array></dict></plist>"#
}

/// REQ-07 / S-05: only the three `tui` keys are edited in place; an unknown
/// id survives at its user-selected position, and stale/unwritable writes do
/// not change the original file.
#[test]
fn test_s05_toml_inplace_edit() {
    let temp = TempDir::new("s05-codex-config");
    let config_path = temp.path().join("config.toml");
    fs::write(&config_path, config_fixture()).expect("write TOML fixture");

    let snapshot = config_io::read_config(&config_path).expect("read editable TOML");
    assert_eq!(
        snapshot.config.status_line,
        [
            "model-with-reasoning",
            "plugin-keeps-its-place",
            "context-used",
            "weekly-limit"
        ],
    );
    let edited = CodexConfig {
        status_line: vec![
            "weekly-limit".into(),
            "plugin-keeps-its-place".into(),
            "context-used".into(),
            "model-with-reasoning".into(),
        ],
        status_line_use_colors: false,
        theme: "Neon-Night".into(),
    };
    config_io::write_config(&snapshot, &edited).expect("write the three TOML keys");
    let written = fs::read_to_string(&config_path).expect("read edited TOML");
    assert!(
        written.contains(
            "# preserve this header exactly\n[other]\nkeep = \"this formatting\" # untouched\n"
        ) && written.contains("\n[future]\nvalue = 7\n"),
        "comments and formatting of all untouched TOML lines remain byte-for-byte present",
    );
    let reloaded = config_io::read_config(&config_path).expect("re-read edited TOML");
    assert_eq!(reloaded.config, edited);
    assert_eq!(
        reloaded.config.status_line[1], "plugin-keeps-its-place",
        "unknown ids are preserved and move only with the requested ordering",
    );

    let stale_snapshot = config_io::read_config(&config_path).expect("capture stale snapshot");
    fs::write(
        &config_path,
        format!(
            "{}# concurrent Codex update\n",
            fs::read_to_string(&config_path).unwrap()
        ),
    )
    .expect("simulate external modification");
    let externally_changed = fs::read(&config_path).expect("read concurrent bytes");
    assert_eq!(
        config_io::write_config(&stale_snapshot, &edited),
        Err(ConfigIoError::AbortStale),
        "a changed file aborts instead of overwriting another Codex instance",
    );
    assert_eq!(
        fs::read(&config_path).unwrap(),
        externally_changed,
        "stale abort performs no write"
    );

    assert_unwritable_write_is_non_destructive(temp.path(), &edited);
}

#[cfg(unix)]
fn assert_unwritable_write_is_non_destructive(temp: &Path, edited: &CodexConfig) {
    use std::os::unix::fs::PermissionsExt;

    let locked = temp.join("locked");
    fs::create_dir(&locked).expect("create locked directory");
    let path = locked.join("config.toml");
    fs::write(&path, config_fixture()).expect("write locked TOML fixture");
    let snapshot = config_io::read_config(&path).expect("read locked TOML fixture");
    let before = fs::read(&path).expect("read locked TOML bytes");
    let original_permissions = fs::metadata(&locked).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_mode(0o555);
    fs::set_permissions(&locked, read_only).expect("make target directory unwritable");
    let result = config_io::write_config(&snapshot, edited);
    fs::set_permissions(&locked, original_permissions)
        .expect("restore target directory permissions");
    assert!(
        result.is_err(),
        "unwritable target directory returns an error"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "failed write leaves original TOML intact"
    );
}

#[cfg(not(unix))]
fn assert_unwritable_write_is_non_destructive(_temp: &Path, _edited: &CodexConfig) {
    panic!("S-05 needs a platform-specific unwritable-directory fixture");
}

/// REQ-07 / R11 / R13 / S-06: a fused real-world-shaped selector initially
/// gives usage and limit values one colour; splitting it permits two colours,
/// while disabled colours produce a wholly dim line.
#[test]
fn test_s06_preview_line_render() {
    let temp = TempDir::new("s06-codex-preview");
    let theme_path = temp.path().join("Matrix-Tron.tmTheme");
    fs::write(&theme_path, fused_tmtheme_fixture()).expect("write fused tmTheme fixture");
    let items = vec![
        "model-with-reasoning".into(),
        "context-used".into(),
        "weekly-limit".into(),
    ];

    let fused = preview::render_preview_line(&items, true, &theme_path, "truecolor")
        .expect("render fused-scope preview");
    let fused_colour = "\x1b[38;2;17;170;34m";
    assert!(
        fused.contains("62%") && fused.contains("78%"),
        "preview contains fixed usage and weekly samples"
    );
    assert_eq!(
        fused.matches(fused_colour).count(),
        2,
        "fused scopes colour both usage and limit alike"
    );

    fs::write(
        &theme_path,
        fused_tmtheme_fixture().replace(
            "constant.numeric, constant.language</string><key>settings</key><dict><key>foreground</key><string>#11AA22</string>",
            "constant.numeric</string><key>settings</key><dict><key>foreground</key><string>#2255EE</string>",
        ).replace(
            "</array></dict></plist>",
            "<dict><key>scope</key><string>constant.language</string><key>settings</key><dict><key>foreground</key><string>#EE5522</string></dict></dict></array></dict></plist>",
        ),
    ).expect("write split tmTheme fixture");
    let split = preview::render_preview_line(&items, true, &theme_path, "truecolor")
        .expect("render split-scope preview");
    assert!(
        split.contains("\x1b[38;2;34;85;238m") && split.contains("\x1b[38;2;238;85;34m"),
        "split scopes render usage and weekly limit in their respective colours"
    );

    let plain = preview::render_preview_line(&items, false, &theme_path, "truecolor")
        .expect("render colour-disabled preview");
    assert!(
        plain.starts_with("\x1b[2m") && plain.ends_with("\x1b[0m"),
        "colour-disabled preview is entirely dim"
    );
    assert!(
        !plain.contains("38;2;"),
        "colour-disabled preview has no foreground colour escapes"
    );
}
