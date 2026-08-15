//! RED coverage for the tmTheme editor's XML-only round-trip and guided split.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "../src/tui/codex/tmtheme.rs"]
mod tmtheme;

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

const CODEX_SCOPES: [&str; 4] = [
    "constant.numeric",
    "constant",
    "constant.language",
    "storage.type",
];

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

fn fused_matrix_tron_fixture() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>name</key><string>Matrix-Tron</string><key>settings</key><array>
<dict><key>settings</key><dict><key>foreground</key><string>#D0D0D0</string><key>background</key><string>#001100</string><key>caret</key><string>#00FF41</string></dict></dict>
<dict><key>name</key><string>Fused Usage and Limit</string><key>scope</key><string>constant.numeric, constant.language</string><key>settings</key><dict><key>foreground</key><string>#11AA22</string><key>fontStyle</key><string>bold</string></dict></dict>
<dict><key>name</key><string>Comment</string><key>scope</key><string>comment</string><key>settings</key><dict><key>foreground</key><string>#667766</string></dict></dict>
</array></dict></plist>"#
}

/// REQ-08 / S-07: XML tmThemes round-trip semantically, while XML parse and
/// write failures remain read-only; splitting a fused entry gives exactly the
/// four documented Codex scopes dedicated, independently editable colours.
#[test]
fn test_s07_tmtheme_roundtrip_split() {
    let temp = TempDir::new("s07-tmtheme");
    let theme_path = temp.path().join("Matrix-Tron.tmTheme");
    fs::write(&theme_path, fused_matrix_tron_fixture()).expect("write fused Matrix-Tron fixture");

    let mut snapshot = tmtheme::read_tmtheme(&theme_path).expect("parse XML tmTheme");
    assert_eq!(snapshot.theme.global_foreground(), Some("#D0D0D0"));
    assert_eq!(snapshot.theme.global_background(), Some("#001100"));
    assert_eq!(
        snapshot.theme.resolved_foreground("storage.type"),
        Some("#D0D0D0")
    );
    let unchanged_comment = snapshot
        .theme
        .resolved_foreground("comment")
        .map(str::to_owned);
    snapshot.theme.set_global_foreground("#00FF41");
    tmtheme::write_tmtheme(&snapshot).expect("write edited XML tmTheme");
    let reloaded = tmtheme::read_tmtheme(&theme_path).expect("re-read edited XML tmTheme");
    assert_eq!(reloaded.theme.global_foreground(), Some("#00FF41"));
    assert_eq!(
        reloaded
            .theme
            .resolved_foreground("comment")
            .map(str::to_owned),
        unchanged_comment,
        "unmodified settings remain semantically equal after plist re-parse",
    );
    assert!(
        fs::read_to_string(&theme_path)
            .expect("read written theme")
            .contains("<plist"),
        "write-back remains a valid XML plist",
    );

    let mut split = reloaded;
    tmtheme::split_codex_scopes(&mut split.theme).expect("split fused Codex scopes");
    for (scope, colour) in CODEX_SCOPES
        .iter()
        .zip(["#1188EE", "#22AA44", "#EE7722", "#CC33AA"])
    {
        split
            .theme
            .set_scope_foreground(scope, colour)
            .expect("edit dedicated Codex scope");
    }
    tmtheme::write_tmtheme(&split).expect("write split XML tmTheme");
    let split_reloaded = tmtheme::read_tmtheme(&theme_path).expect("re-read split XML tmTheme");
    for (scope, colour) in CODEX_SCOPES
        .iter()
        .zip(["#1188EE", "#22AA44", "#EE7722", "#CC33AA"])
    {
        assert_eq!(
            split_reloaded.theme.resolved_foreground(scope),
            Some(colour),
            "{scope} resolves to its dedicated split colour"
        );
    }
    assert_eq!(
        split_reloaded
            .theme
            .resolved_foreground("constant.numeric.extra"),
        Some("#1188EE"),
        "longest-prefix matching selects the dedicated numeric entry",
    );

    let binary_path = temp.path().join("binary.tmTheme");
    fs::write(&binary_path, b"bplist00\xd1\x01\x02\x03\x04").expect("write binary plist fixture");
    assert!(
        tmtheme::read_tmtheme(&binary_path).is_err(),
        "binary plist is rejected as read-only"
    );

    let malformed_path = temp.path().join("malformed.tmTheme");
    fs::write(&malformed_path, "<plist><dict><key>settings</key>")
        .expect("write malformed XML fixture");
    assert!(
        tmtheme::read_tmtheme(&malformed_path).is_err(),
        "malformed XML is rejected as read-only"
    );

    assert_unwritable_write_is_non_destructive(temp.path());
}

#[cfg(unix)]
fn assert_unwritable_write_is_non_destructive(temp: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let locked = temp.join("locked");
    fs::create_dir(&locked).expect("create locked directory");
    let path = locked.join("Matrix-Tron.tmTheme");
    fs::write(&path, fused_matrix_tron_fixture()).expect("write locked tmTheme fixture");
    let snapshot = tmtheme::read_tmtheme(&path).expect("read locked tmTheme fixture");
    let before = fs::read(&path).expect("read locked tmTheme bytes");
    let original_permissions = fs::metadata(&locked)
        .expect("inspect locked directory")
        .permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_mode(0o555);
    fs::set_permissions(&locked, read_only).expect("make target directory unwritable");
    let result = tmtheme::write_tmtheme(&snapshot);
    fs::set_permissions(&locked, original_permissions)
        .expect("restore target directory permissions");
    assert!(
        result.is_err(),
        "unwritable target directory returns an error"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "failed write leaves original tmTheme intact"
    );
}

#[cfg(not(unix))]
fn assert_unwritable_write_is_non_destructive(_temp: &Path) {
    panic!("S-07 needs a platform-specific unwritable-directory fixture");
}
