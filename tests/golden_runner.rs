use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-s01-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
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

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn first_difference(expected: &[u8], actual: &[u8]) -> Option<usize> {
    let common = expected.len().min(actual.len());
    expected[..common]
        .iter()
        .zip(&actual[..common])
        .position(|(left, right)| left != right)
        .or_else(|| (expected.len() != actual.len()).then_some(common))
}

fn hex_window(bytes: &[u8], offset: usize) -> String {
    let start = offset.saturating_sub(8);
    let end = (offset + 8).min(bytes.len());
    bytes[start..end]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_sample(sample: &Path) -> Result<(), String> {
    let name = sample
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "sample name is not valid UTF-8".to_owned())?;
    let meta: serde_json::Value = serde_json::from_slice(
        &fs::read(sample.join("meta.json")).map_err(|error| format!("read meta.json: {error}"))?,
    )
    .map_err(|error| format!("parse meta.json: {error}"))?;
    let mode = meta["mode"]
        .as_str()
        .ok_or_else(|| "meta.json mode must be a string".to_owned())?;
    if !matches!(mode, "render" | "subagent") {
        return Err(format!("unsupported mode {mode:?}"));
    }
    let env: BTreeMap<String, serde_json::Value> = serde_json::from_slice(
        &fs::read(sample.join("env.json")).map_err(|error| format!("read env.json: {error}"))?,
    )
    .map_err(|error| format!("parse env.json: {error}"))?;
    let env_string = |key: &str| -> Result<String, String> {
        env.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("env.json {key} must be a string"))
    };
    let now_ms = env
        .get("now_ms")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| "env.json now_ms must be an integer".to_owned())?;

    let temp = TempDir::new();
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    fs::create_dir_all(&home).map_err(|error| format!("create HOME: {error}"))?;
    fs::create_dir_all(&config).map_err(|error| format!("create config: {error}"))?;
    fs::copy(sample.join("settings.json"), config.join("settings.json"))
        .map_err(|error| format!("copy settings.json: {error}"))?;
    let state = sample.join("state");
    if state.is_dir() {
        copy_tree(&state, &config).map_err(|error| format!("copy state fixture: {error}"))?;
    }

    let bin = fs::canonicalize(sample.join("bin")).map_err(|error| format!("resolve bin: {error}"))?;
    for entry in fs::read_dir(&bin).map_err(|error| format!("read bin: {error}"))? {
        let executable = entry.map_err(|error| format!("read bin entry: {error}"))?.path();
        let status = Command::new(&executable)
            .env_clear()
            .env("PATH", &bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("warm {}: {error}", executable.display()))?;
        if !status.success() {
            return Err(format!("warm {} exited {status}", executable.display()));
        }
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .env_clear()
        .current_dir(sample.join("cwd"))
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("PPULSE_CONFIG_DIR", &config)
        .env("PPULSE_NOW_MS", now_ms.to_string())
        .env("COLUMNS", env_string("COLUMNS")?)
        .env("TERM", env_string("TERM")?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(value) = env.get("COLORTERM").and_then(serde_json::Value::as_str) {
        command.env("COLORTERM", value);
    }
    for (key, value) in &env {
        if key.starts_with("PPULSE_") {
            let value = value
                .as_str()
                .ok_or_else(|| format!("env.json {key} must be a string"))?;
            command.env(key, value);
        }
    }
    match mode {
        "render" => command.arg("render"),
        "subagent" => command.args(["render", "--subagent"]),
        _ => unreachable!(),
    };
    let mut child = command.spawn().map_err(|error| format!("spawn: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "open stdin".to_owned())?
        .write_all(&fs::read(sample.join("stdin.json")).map_err(|error| format!("read stdin.json: {error}"))?)
        .map_err(|error| format!("write stdin: {error}"))?;
    let output = child.wait_with_output().map_err(|error| format!("wait: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "exit {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim_end()
        ));
    }
    let expected = fs::read(sample.join("expected.bin")).map_err(|error| format!("read expected.bin: {error}"))?;
    if let Some(offset) = first_difference(&expected, &output.stdout) {
        return Err(format!(
            "byte mismatch at offset {offset}: expected [{}], actual [{}]",
            hex_window(&expected, offset),
            hex_window(&output.stdout, offset),
        ));
    }
    println!("golden {name}: matched");
    Ok(())
}

// REQ-02 / S-01: every hermetic sample must match the frozen TS stdout byte-for-byte.
#[test]
fn test_s01_all_golden_samples() {
    let mut samples = fs::read_dir("tests/golden")
        .expect("read golden sample directory")
        .map(|entry| entry.expect("read golden sample entry"))
        .filter(|entry| entry.file_type().expect("read golden sample type").is_dir())
        .collect::<Vec<_>>();
    samples.sort_by_key(|entry| entry.file_name());

    let total = samples.len();
    let mut matched = 0;
    let mut failures = Vec::new();
    for sample in samples {
        let name = sample.file_name().to_string_lossy().into_owned();
        match run_sample(&sample.path()) {
            Ok(()) => matched += 1,
            Err(error) => {
                println!("golden {name}: FAILED: {error}");
                failures.push(format!("{name}: {error}"));
            }
        }
    }
    println!("golden tally: {matched}/{total} matched");
    assert!(failures.is_empty(), "golden failures:\n{}", failures.join("\n"));
}
