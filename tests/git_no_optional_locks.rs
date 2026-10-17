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
        let path = std::env::temp_dir().join(format!(
            "phosphorpulse-nolock-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        Self(path)
    }
    fn path(&self) -> &Path { &self.0 }
}

impl Drop for TempDir {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn input_with_cwd(cwd: &Path) -> Vec<u8> {
    let stdin = fs::read("tests/golden/main-default/stdin.json").expect("read render stdin");
    let mut input: serde_json::Value = serde_json::from_slice(&stdin).expect("parse render stdin");
    input["cwd"] = serde_json::json!(cwd);
    serde_json::to_vec(&input).expect("serialize render stdin")
}

fn render(config_dir: &Path, input: &[u8], extra: impl FnOnce(&mut Command)) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    child.arg("render").env("PPULSE_CONFIG_DIR", config_dir)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    extra(&mut child);
    let mut child = child.spawn().expect("start render");
    child.stdin.take().expect("render stdin").write_all(input).expect("write render stdin");
    child.wait_with_output().expect("wait for render")
}

// Records `ARGS=<argv>` and `LOCKS=<GIT_OPTIONAL_LOCKS or "unset">` per call,
// then prints valid porcelain output.
fn write_recording_git_stub(path: &Path) {
    fs::write(
        path,
        "#!/bin/sh\n\
         printf 'ARGS=%s\\n' \"$*\" >> \"$PPULSE_GIT_RECORD\"\n\
         printf 'LOCKS=%s\\n' \"${GIT_OPTIONAL_LOCKS-unset}\" >> \"$PPULSE_GIT_RECORD\"\n\
         printf '## main...origin/main\\n'\n",
    )
    .expect("write git stub");
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("stub metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make stub executable");
    }
}

#[test]
fn test_g01_git_segment_disables_optional_locks() {
    let temp = TempDir::new("g01");
    let config_dir = temp.path().join("config");
    let home_dir = temp.path().join("home");
    let cwd = temp.path().join("cwd");
    let stub_bin = temp.path().join("bin");
    let record = temp.path().join("record");
    for dir in [&config_dir, &home_dir, &cwd, &stub_bin] {
        fs::create_dir_all(dir).expect("create temp subdirectory");
    }
    fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&serde_json::json!({"rows": [{"segments": ["git"]}]})).expect("serialize settings"),
    )
    .expect("write settings");
    write_recording_git_stub(&stub_bin.join("git"));

    // Warm the stub so first-exec latency cannot trip the lookup timeout.
    assert!(Command::new(stub_bin.join("git")).env("PPULSE_GIT_RECORD", temp.path().join("warm-record"))
        .stdout(Stdio::null()).status().expect("warm stub").success());

    let path = format!("{}:{}", stub_bin.display(), std::env::var("PATH").expect("PATH set"));
    let output = render(&config_dir, &input_with_cwd(&cwd), |command| {
        command.env("PATH", path).env("HOME", &home_dir).env("PPULSE_GIT_RECORD", &record)
            .env_remove("GIT_OPTIONAL_LOCKS");
    });
    assert!(output.status.success(), "git render failed: {:?}", output.stderr);

    assert!(record.exists(), "git stub was not invoked (no record file)");
    let recorded = fs::read_to_string(&record).expect("read git record");
    let args = recorded.lines().find_map(|line| line.strip_prefix("ARGS=")).expect("recorded args");
    let locks = recorded.lines().find_map(|line| line.strip_prefix("LOCKS=")).expect("recorded env");

    let words: Vec<_> = args.split_whitespace().collect();
    let status_at = words.iter().position(|word| *word == "status").expect("git status invoked");
    let flag_before_status = words[..status_at].contains(&"--no-optional-locks");
    assert!(flag_before_status || locks == "0",
        "git segment must disable optional locks (--no-optional-locks before `status` or GIT_OPTIONAL_LOCKS=0); \
         got args={args:?} GIT_OPTIONAL_LOCKS={locks:?}");
}
