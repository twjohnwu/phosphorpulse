use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

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

fn write_valid_settings(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).expect("create config directory");
    let settings = dir.join("settings.json");
    fs::copy("tests/golden/main-default/settings.json", &settings)
        .expect("copy valid golden settings");
    settings
}

fn run_config(
    home: &Path,
    ppulse_config_dir: Option<&Path>,
    ppf_config_dir: Option<&Path>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .arg("config")
        .env("HOME", home)
        .env_remove("PPULSE_CONFIG_DIR")
        .env_remove("PPF_CONFIG_DIR");
    if let Some(path) = ppulse_config_dir {
        command.env("PPULSE_CONFIG_DIR", path);
    }
    if let Some(path) = ppf_config_dir {
        command.env("PPF_CONFIG_DIR", path);
    }
    command.output().expect("run phosphorpulse config")
}

/// REQ-01 / REQ-03 / S-02: config resolution, fail-loud, and subagent fallback.
#[test]
fn test_s02_config_resolution_and_fallback() {
    let temp = TempDir::new("s02-config");
    let home = temp.path().join("home");
    let default_config_dir = home.join(".claude/phosphorpulse");
    let default_settings = write_valid_settings(&default_config_dir);

    // (a) Default resolution also reports effective values and their source.
    let default_output = run_config(&home, None, None);
    assert!(
        default_output.status.success(),
        "default config command must exit 0"
    );
    let default_stdout = String::from_utf8(default_output.stdout).expect("UTF-8 config output");
    assert!(default_stdout.contains(default_config_dir.to_string_lossy().as_ref()));
    assert!(default_stdout.contains(default_settings.to_string_lossy().as_ref()));
    assert!(default_stdout.contains("activeTemplate"));
    assert!(default_stdout.contains("matrix-tron"));
    assert!(default_stdout.contains("source"));

    // (b) PPULSE_CONFIG_DIR wins; the legacy PPF_CONFIG_DIR is ignored.
    let custom_config_dir = temp.path().join("custom-config");
    let custom_settings = write_valid_settings(&custom_config_dir);
    let ignored_ppf_dir = temp.path().join("ignored-ppf-config");
    write_valid_settings(&ignored_ppf_dir);
    let custom_output = run_config(&home, Some(&custom_config_dir), Some(&ignored_ppf_dir));
    assert!(
        custom_output.status.success(),
        "custom config command must exit 0"
    );
    let custom_stdout = String::from_utf8(custom_output.stdout).expect("UTF-8 config output");
    assert!(custom_stdout.contains(custom_config_dir.to_string_lossy().as_ref()));
    assert!(custom_stdout.contains(custom_settings.to_string_lossy().as_ref()));
    assert!(!custom_stdout.contains(ignored_ppf_dir.to_string_lossy().as_ref()));

    // (c) Invalid config fails loudly for render, but subagent falls back to defaults.
    let invalid_config_dir = temp.path().join("invalid-config");
    fs::create_dir_all(&invalid_config_dir).expect("create invalid config directory");
    fs::write(invalid_config_dir.join("settings.json"), b"{ invalid JSON")
        .expect("write invalid config");
    let stdin = fs::read("tests/golden/main-default/stdin.json").expect("read valid golden stdin");

    let render_output = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("render")
        .env("HOME", &home)
        .env("PPULSE_CONFIG_DIR", &invalid_config_dir)
        .env_remove("PPF_CONFIG_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render");
    let mut render_child = render_output;
    use std::io::Write;
    render_child
        .stdin
        .take()
        .expect("render stdin")
        .write_all(&stdin)
        .expect("write render stdin");
    let render_output = render_child.wait_with_output().expect("wait for render");
    assert!(
        render_output.status.success(),
        "invalid-config render must exit 0"
    );
    let render_stdout = String::from_utf8(render_output.stdout).expect("UTF-8 render output");
    assert!(
        render_stdout.contains("warning"),
        "invalid config must produce a fail-loud warning"
    );
    assert_eq!(
        render_stdout.lines().count(),
        1,
        "invalid config must not render a main line"
    );

    let mut subagent_child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .args(["render", "--subagent"])
        .env("HOME", &home)
        .env("PPULSE_CONFIG_DIR", &invalid_config_dir)
        .env_remove("PPF_CONFIG_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse render --subagent");
    subagent_child
        .stdin
        .take()
        .expect("subagent stdin")
        .write_all(&stdin)
        .expect("write subagent stdin");
    let subagent_output = subagent_child
        .wait_with_output()
        .expect("wait for subagent render");
    assert!(
        subagent_output.status.success(),
        "subagent fallback must exit 0"
    );
    assert!(
        !subagent_output.stdout.is_empty(),
        "subagent fallback must render default output"
    );
}
