use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
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

fn valid_config_dir(temp: &TempDir) -> PathBuf {
    let config_dir = temp.path().join("config");
    fs::create_dir_all(&config_dir).expect("create config directory");
    fs::copy(
        "tests/golden/main-default/settings.json",
        config_dir.join("settings.json"),
    )
    .expect("copy valid settings");
    config_dir
}

fn run_with_stdin(args: &[&str], config_dir: &Path, stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .args(args)
        .env("PPULSE_CONFIG_DIR", config_dir)
        .env_remove("PPF_CONFIG_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait for phosphorpulse")
}

fn assert_stdin_error(output: Output, label: &str) {
    assert_eq!(output.status.code(), Some(1), "{label}: exit code");
    assert!(output.stdout.is_empty(), "{label}: stdout must be empty");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert_eq!(
        stderr.lines().count(),
        1,
        "{label}: stderr must be one line"
    );
    assert!(
        stderr.contains("stdin"),
        "{label}: stderr must mention stdin"
    );
}

fn assert_bom_crash_parity(output: Output, label: &str) {
    assert_eq!(output.status.code(), Some(1), "{label}: exit code");
    assert!(output.stdout.is_empty(), "{label}: stdout must be empty");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert_eq!(
        stderr.lines().count(),
        1,
        "{label}: stderr must be one line"
    );
}

// REQ-01 / S-03: malformed stdin follows deviation 5. The no-args guard below is
// REQ-level only (no S-XX): the fingerprint-locked spec reserves TUI for later work.
#[test]
fn test_s03_malformed_stdin() {
    let temp = TempDir::new("s03-stdin");
    let config_dir = valid_config_dir(&temp);

    for (args, command_name) in [
        (["render"].as_slice(), "render"),
        (["render", "--subagent"].as_slice(), "render --subagent"),
    ] {
        assert_stdin_error(
            run_with_stdin(args, &config_dir, b""),
            &format!("{command_name}, empty stdin"),
        );
        assert_stdin_error(
            run_with_stdin(args, &config_dir, b"not json"),
            &format!("{command_name}, non-JSON stdin"),
        );
    }

    let mut bom_stdin = b"\xef\xbb\xbf".to_vec();
    bom_stdin.extend(fs::read("tests/golden/main-default/stdin.json").expect("read golden stdin"));
    // Frozen TS empirical result (2026-08-14): BOM input exited 1 with empty stdout.
    for (args, command_name) in [
        (["render"].as_slice(), "render"),
        (["render", "--subagent"].as_slice(), "render --subagent"),
    ] {
        assert_bom_crash_parity(
            run_with_stdin(args, &config_dir, &bom_stdin),
            &format!("{command_name}, BOM stdin"),
        );
    }
}

#[test]
fn test_req01_no_args_exit1() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start phosphorpulse without arguments");
    drop(child.stdin.take()); // Close stdin immediately: this must not wait for input.
    let output = child.wait_with_output().expect("wait for no-args command");

    assert_eq!(output.status.code(), Some(1), "no-args exit code");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(!stdout.is_empty(), "no-args explanation must not be empty");
    assert!(
        (stdout.contains("TUI") && stdout.to_lowercase().contains("not implemented"))
            || stdout.contains("settings.json"),
        "no-args explanation must mention the unimplemented TUI or settings.json"
    );
}
