//! REQ-02 / S-05: `phosphorpulse cmd-refresh <name> <cwd>` end-to-end via the real binary.
//! REQ-03 / S-04: `render`-side trigger and claim (spec.md S-04), via the real binary too.
//! GIVEN follows S-04's shared environment (spec.md S-04); harness idiom mirrors
//! `tests/usage_refresh.rs:588-610` (`env_clear()` + explicit PATH/HOME/PPULSE_CONFIG_DIR/
//! PPULSE_NOW_MS, piped stdin).

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{
    cmd::{self, CommandCache, cache},
    config::model::Config,
};
use serde_json::{Value, json};

const NOW: i64 = 1_789_700_000_000;

static TEMP_DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "cmd_refresh_{label}_{}_{}_{}",
            std::process::id(),
            TEMP_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create test directory");
        // macOS `$TMPDIR` is a symlink into `/private/...`; canonicalize so the string
        // handed to the binary as argv `<cwd>` matches the realpath the child observes.
        Self(path.canonicalize().expect("canonicalize test directory"))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn as_str(&self) -> String {
        self.0.to_str().expect("temp dir path is utf-8").to_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A path that is guaranteed not to exist on disk (case 9's `Q`).
fn missing_dir_path(label: &str) -> String {
    let unique = format!(
        "cmd_refresh_missing_{label}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos(),
    );
    std::env::temp_dir()
        .join(unique)
        .to_str()
        .expect("temp dir path is utf-8")
        .to_owned()
}

fn write_settings(config_dir: &Path, command: &str, timeout_ms: Option<u64>) {
    let mut config = Value::Object(Config::defaults().0);
    let mut spec = json!({"command": command, "ttlSec": 5});
    if let Some(timeout_ms) = timeout_ms {
        spec["timeoutMs"] = json!(timeout_ms);
    }
    config["commands"] = json!({"k8s": spec});
    fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&config).expect("serialize settings.json fixture"),
    )
    .expect("write settings.json fixture");
}

fn seed_claim(config_dir: &Path, key: &str, command: &str) {
    let cache = CommandCache {
        fetched_at: Some(NOW - 600_000),
        next_fetch_at: NOW + 15_000,
        command: command.to_owned(),
        output: Some("old".into()),
        last_error: None,
    };
    let path = cache::cache_path(config_dir, key);
    fs::create_dir_all(path.parent().expect("cache path has a parent"))
        .expect("create commands directory");
    fs::write(
        &path,
        serde_json::to_vec(&cache).expect("serialize seeded cache"),
    )
    .expect("seed cache fixture");
}

fn write_lock(config_dir: &Path, key: &str, content: &str) {
    let path = cache::lock_path(config_dir, key);
    fs::create_dir_all(path.parent().expect("lock path has a parent"))
        .expect("create commands directory");
    fs::write(&path, content).expect("seed lock fixture");
}

fn read_cache_bytes(config_dir: &Path, key: &str) -> Vec<u8> {
    fs::read(cache::cache_path(config_dir, key)).expect("read cache fixture bytes")
}

fn read_cache_json(config_dir: &Path, key: &str) -> Value {
    serde_json::from_slice(&read_cache_bytes(config_dir, key)).expect("parse cache fixture")
}

fn stdin_json(cwd: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"cwd": cwd, "model": {"display_name": "Probe"}}))
        .expect("serialize S-04 stdin payload")
}

struct RunResult {
    output: Output,
    elapsed: Duration,
}

fn run_cmd_refresh(
    config_dir: &Path,
    name: &str,
    cwd_arg: &str,
    stdin_bytes: Vec<u8>,
) -> RunResult {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("cmd-refresh")
        .arg(name)
        .arg(cwd_arg)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", config_dir)
        .env("PPULSE_CONFIG_DIR", config_dir)
        .env("PPULSE_NOW_MS", NOW.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cmd-refresh");
    let mut stdin = child.stdin.take().expect("cmd-refresh stdin");
    let writer = thread::spawn(move || {
        // The user's command may never read stdin (e.g. case 13's `date`), so a
        // BrokenPipe/EPIPE here is expected and must not be treated as a failure.
        let _ = stdin.write_all(&stdin_bytes);
    });
    let start = Instant::now();
    let output = child.wait_with_output().expect("wait for cmd-refresh");
    let elapsed = start.elapsed();
    let _ = writer.join();
    RunResult { output, elapsed }
}

fn assert_empty_stdio(result: &RunResult, label: &str) {
    assert!(
        result.output.stdout.is_empty(),
        "S-05 case {label}: stdout must be empty, got {:?}",
        result.output.stdout
    );
    assert!(
        result.output.stderr.is_empty(),
        "S-05 case {label}: stderr must be empty, got {:?}",
        result.output.stderr
    );
}

fn assert_lock_released_and_not_reclaimed(config_dir: &Path, key: &str, label: &str) {
    assert!(
        !cache::lock_path(config_dir, key).exists(),
        "S-05 case {label}: lock file must not exist after cmd-refresh exits"
    );
    let cache = read_cache_json(config_dir, key);
    assert_ne!(
        cache["nextFetchAt"],
        json!(NOW + 15_000),
        "S-05 case {label}: nextFetchAt must not be left at the render claim value"
    );
}

/// Bounded deadline for `assert_pid_dead_or_zombie`'s poll — matches `S04_POLL_TIMEOUT`'s
/// order of magnitude for the same reason (a loaded CI runner needs headroom that a local
/// run never notices).
const PID_DEAD_POLL_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Polls until `pid` is either gone from the process table (`kill -0` fails) or a zombie
/// (`ps -o stat=` reports a leading `Z`), up to `PID_DEAD_POLL_TIMEOUT`. Both outcomes mean
/// the process-group kill landed; `kill -0` alone cannot tell the two apart from "still
/// running", because a zombie still answers `kill -0` successfully until its parent (here,
/// an OS-scheduled reparent-and-reap once `sh` dies, not something `cmd-refresh` controls
/// for a grandchild it never directly parented) reaps it.
fn assert_pid_dead_or_zombie(pid: &str, label: &str) {
    let start = Instant::now();
    loop {
        let alive = Command::new("/bin/kill")
            .args(["-0", pid])
            .status()
            .expect("run kill -0 probe")
            .success();
        if !alive {
            return;
        }
        let stat = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .expect("run ps stat probe");
        if String::from_utf8_lossy(&stat.stdout).trim_start().starts_with('Z') {
            return;
        }
        if start.elapsed() >= PID_DEAD_POLL_TIMEOUT {
            panic!(
                "S-05 case {label}: grandchild pid {pid} must be dead (or a zombie) after \
                 process-group kill, within {PID_DEAD_POLL_TIMEOUT:?}"
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn cache_file_mode(config_dir: &Path, key: &str) -> u32 {
    fs::metadata(cache::cache_path(config_dir, key))
        .expect("cache file metadata")
        .permissions()
        .mode()
        & 0o777
}

/// REQ-02 / S-05
#[test]
fn test_s05_refresh_subcommand() {
    // (1) success: exit 0, output is the command's first stdout line, fetchedAt=now,
    // nextFetchAt=now+ttlSec*1000 (ttlSec=5), cache file mode 0600, lock released.
    {
        let label = "1";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "printf 'line1\\nline2'";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(cache["output"], json!("line1"), "S-05 case {label} output");
        assert_eq!(cache["fetchedAt"], json!(NOW), "S-05 case {label} fetchedAt");
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 5_000),
            "S-05 case {label} nextFetchAt"
        );
        assert_eq!(
            cache_file_mode(d.path(), &key),
            0o600,
            "S-05 case {label} cache file mode"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (2) non-zero exit: exit 4, old output/fetchedAt kept, nextFetchAt=now+30_000.
    {
        let label = "2";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "exit 3";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(4), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(cache["output"], json!("old"), "S-05 case {label} output");
        assert_eq!(
            cache["fetchedAt"],
            json!(NOW - 600_000),
            "S-05 case {label} fetchedAt"
        );
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 30_000),
            "S-05 case {label} nextFetchAt"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (3) timeout kills the whole process group: exit 4, elapsed < 1_500ms,
    // nextFetchAt=now+30_000, and the grandchild recorded in gc.pid is dead afterwards.
    {
        let label = "3";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "sh -c 'sleep 30 & echo $! > gc.pid; wait'";
        write_settings(d.path(), command, Some(200));
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(4), "S-05 case {label} exit code");
        assert!(
            result.elapsed < Duration::from_millis(1_500),
            "S-05 case {label} elapsed {:?} must be < 1_500ms",
            result.elapsed
        );
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 30_000),
            "S-05 case {label} nextFetchAt"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);

        let pid_path = p.path().join("gc.pid");
        let pid = fs::read_to_string(&pid_path)
            .unwrap_or_default()
            .trim()
            .to_owned();
        assert!(
            !pid.is_empty(),
            "S-05 case {label}: gc.pid must have been written before the kill"
        );
        // `kill -0` alone is not sufficient: a SIGKILLed process is a zombie (and
        // `kill -0` on a zombie still succeeds) until its parent gets reparented and
        // reaped, which is an OS-scheduled event `cmd-refresh` does not control for a
        // grandchild it never directly parented. Poll for either "no such process" or
        // an observed zombie (`ps -o stat=` reports `Z`) — both mean the kill landed;
        // only a live, non-zombie state after the deadline is a real failure.
        assert_pid_dead_or_zombie(&pid, label);
    }

    // (4) undeclared name: exit 2, cache bytes untouched (never read).
    {
        let label = "4";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_settings(d.path(), "true", None);
        let key = cmd::cache_key("nope", Some(&p.as_str()));
        seed_claim(d.path(), &key, "true");
        let before = read_cache_bytes(d.path(), &key);

        let result = run_cmd_refresh(d.path(), "nope", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(2), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        assert_eq!(
            read_cache_bytes(d.path(), &key),
            before,
            "S-05 case {label}: cache bytes must be unchanged"
        );
    }

    // (5) lock already held: exit 5, cache bytes unchanged, lock unchanged.
    {
        let label = "5";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "true";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);
        write_lock(d.path(), &key, "other");
        let before = read_cache_bytes(d.path(), &key);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(5), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        assert_eq!(
            read_cache_bytes(d.path(), &key),
            before,
            "S-05 case {label}: cache bytes must be unchanged"
        );
        assert_eq!(
            fs::read_to_string(cache::lock_path(d.path(), &key)).expect("read lock file"),
            "other",
            "S-05 case {label}: lock file content must be unchanged"
        );
    }

    // (6) output at the 4_096-byte cap: exit 0, output.len()==4_096.
    {
        let label = "6";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "printf '%05000d' 0";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(
            cache["output"].as_str().expect("output is a string").len(),
            4_096,
            "S-05 case {label}: output must be truncated to 4_096 bytes"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (7) no output at all: exit 4 (empty stdout is a failure even on exit 0), old kept.
    {
        let label = "7";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "printf ''";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(4), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(cache["output"], json!("old"), "S-05 case {label} output");
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (8) stdin feed truncated at 1 MiB: exit 0, output=="1048576".
    {
        let label = "8";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "wc -c | tr -d ' '";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);
        let stdin_bytes = vec![b'x'; 1_048_577];

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_bytes);

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(
            cache["output"],
            json!("1048576"),
            "S-05 case {label}: stdin feed must be capped at 1 MiB"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (9) argv cwd does not exist: exit 4, cache under key (k8s, Q), Q never created.
    {
        let label = "9";
        let d = TempDir::new(&format!("d{label}"));
        let q = missing_dir_path(label);
        assert!(!Path::new(&q).exists(), "S-05 case {label}: Q must not pre-exist");
        let command = "true";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&q));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &q, stdin_json(&q));

        assert_eq!(result.output.status.code(), Some(4), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 30_000),
            "S-05 case {label} nextFetchAt"
        );
        assert!(
            !Path::new(&q).exists(),
            "S-05 case {label}: Q must not have been created"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (10) invalid UTF-8 stdout is lossily replaced: exit 0.
    {
        let label = "10";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "printf '\\377\\376ok'";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(
            cache["output"],
            json!("\u{FFFD}\u{FFFD}ok"),
            "S-05 case {label} output"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (11) multi-byte UTF-8 truncation stays on a char boundary: exit 0.
    {
        let label = "11";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "awk 'BEGIN{for(i=0;i<2000;i++)printf \"\u{65e5}\"}'";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        let output = cache["output"].as_str().expect("output is a string");
        assert!(
            output.len() <= 4_096,
            "S-05 case {label}: output must be at most 4_096 bytes, got {}",
            output.len()
        );
        assert!(
            output.chars().all(|ch| ch == '\u{65e5}'),
            "S-05 case {label}: output must be entirely U+65E5 characters"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (12) output hits the 65_536-byte read cap regardless of the pipeline's own exit
    // code: exit 4, old kept.
    {
        let label = "12";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "yes x | head -c 70000";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(4), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(cache["output"], json!("old"), "S-05 case {label} output");
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }

    // (13) command never reads stdin: BrokenPipe from the forwarding thread must not be
    // treated as a failure. exit 0, output is a two-digit hour.
    {
        let label = "13";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        let command = "date +%H";
        write_settings(d.path(), command, None);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_claim(d.path(), &key, command);

        let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

        assert_eq!(result.output.status.code(), Some(0), "S-05 case {label} exit code");
        assert_empty_stdio(&result, label);
        let cache = read_cache_json(d.path(), &key);
        let output = cache["output"].as_str().expect("output is a string");
        assert_eq!(output.len(), 2, "S-05 case {label}: output must be a two-digit hour");
        assert!(
            output.chars().all(|ch| ch.is_ascii_digit()),
            "S-05 case {label}: output must be numeric, got {output:?}"
        );
        assert_lock_released_and_not_reclaimed(d.path(), &key, label);
    }
}

/// Regression test for a review finding (not an S-XX scenario): when the shell exits
/// early while a grandchild keeps the stdout pipe open (`sleep 60 &` with no `wait`,
/// unlike S-05 case 3's `wait`-based shape), the worker thread's `try_wait` succeeds as
/// soon as the shell exits and must not empty the shared `Child` handle before the
/// timeout branch might still need it to kill the process group. Confirms the grandchild
/// is gone (or a zombie) after `cmd-refresh` exits, using the same
/// `assert_pid_dead_or_zombie` poll as S-05 case 3.
#[test]
fn test_pgroup_kill_when_shell_exits_early() {
    let d = TempDir::new("leaky_d");
    let p = TempDir::new("leaky_p");
    let label = "leaky";
    let command = "sleep 60 & echo $! > gc.pid; printf ok";
    write_settings(d.path(), command, Some(300));
    let key = cmd::cache_key("k8s", Some(&p.as_str()));
    seed_claim(d.path(), &key, command);

    let result = run_cmd_refresh(d.path(), "k8s", &p.as_str(), stdin_json(&p.as_str()));

    // Observed: the grandchild keeps the stdout pipe open past the shell's exit, so the
    // read never sees EOF before `timeoutMs` fires — same outer failure shape as S-05
    // case 3 (timeout kill): exit 4, old cache kept, nextFetchAt bumped by the timeout
    // path's `now+30_000`, lock released.
    assert_eq!(result.output.status.code(), Some(4), "case {label} exit code");
    assert_empty_stdio(&result, label);
    let cache = read_cache_json(d.path(), &key);
    assert_eq!(cache["output"], json!("old"), "case {label} output");
    assert_eq!(
        cache["nextFetchAt"],
        json!(NOW + 30_000),
        "case {label} nextFetchAt"
    );
    assert_lock_released_and_not_reclaimed(d.path(), &key, label);

    let pid_path = p.path().join("gc.pid");
    let pid = fs::read_to_string(&pid_path)
        .unwrap_or_default()
        .trim()
        .to_owned();
    assert!(!pid.is_empty(), "case {label}: gc.pid must have been written before the kill");
    assert_pid_dead_or_zombie(&pid, label);
}

// ---------------------------------------------------------------------------------------
// REQ-03 / S-04: render-side trigger and claim.
// ---------------------------------------------------------------------------------------

/// Shell command shared by S-04 cases (1)(3)(6)(7): forwards raw stdin to `seen.json`,
/// copies the (possibly claimed) cache file into `cache-at-run.json` while the child is
/// still running, then emits `hello-<basename of $PWD>` as its only stdout line. Uses
/// relative paths because `run_with_stdin` sets `current_dir` to `P` (spec.md S-04 GIVEN).
const S04_COMMAND: &str = "cat > seen.json; cp \"$PPULSE_CONFIG_DIR\"/commands/k8s-*.json cache-at-run.json; printf %s hello-$(basename \"$PWD\")";

const S04_POLL_TIMEOUT: Duration = Duration::from_millis(2_000);

fn write_render_settings(config_dir: &Path, command: &str, ttl_sec: u64, with_cmd_row: bool) {
    let mut config = Value::Object(Config::defaults().0);
    config["rows"] = if with_cmd_row {
        json!([{"layout": "fixed", "segments": ["cmd:k8s"]}])
    } else {
        json!([])
    };
    config["commands"] = json!({"k8s": {"command": command, "ttlSec": ttl_sec, "maxWidth": 80}});
    config["segments"] = json!({"cmd:k8s": {"fg": "#00CDCD"}});
    fs::write(
        config_dir.join("settings.json"),
        serde_json::to_vec(&config).expect("serialize S-04 render settings fixture"),
    )
    .expect("write S-04 render settings fixture");
}

fn seed_cache_state(config_dir: &Path, key: &str, cache: &CommandCache) {
    let path = cache::cache_path(config_dir, key);
    fs::create_dir_all(path.parent().expect("cache path has a parent"))
        .expect("create commands directory");
    fs::write(
        &path,
        serde_json::to_vec(cache).expect("serialize S-04 seeded cache"),
    )
    .expect("seed S-04 cache fixture");
}

fn basename(path: &Path) -> String {
    path.file_name()
        .expect("path has a final component")
        .to_str()
        .expect("path is utf-8")
        .to_owned()
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn render_output_text(result: &RunResult) -> String {
    strip_ansi(&String::from_utf8_lossy(&result.output.stdout))
}

/// Mirrors `tests/usage_refresh.rs:588-610`'s `run_render`: real binary, `env_clear` +
/// explicit PATH/HOME/PPULSE_CONFIG_DIR/PPULSE_NOW_MS/COLUMNS, piped stdin.
fn run_render(config_dir: &Path, stdin_bytes: &[u8]) -> RunResult {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"))
        .arg("render")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", config_dir)
        .env("PPULSE_CONFIG_DIR", config_dir)
        .env("PPULSE_NOW_MS", NOW.to_string())
        .env("COLUMNS", "120")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn render");
    child
        .stdin
        .take()
        .expect("render stdin")
        .write_all(stdin_bytes)
        .expect("write render stdin");
    let start = Instant::now();
    let output = child.wait_with_output().expect("wait for render");
    let elapsed = start.elapsed();
    RunResult { output, elapsed }
}

/// Warms up the real binary once (no `cmd:` rows, so no `cmd::segment` lookup happens) to
/// absorb macOS first-exec latency before a case that asserts `elapsed < 300ms`
/// (spec.md S-04 GIVEN note).
fn warmup_render(config_dir: &Path, cwd: &str) {
    write_render_settings(config_dir, "true", 5, false);
    let _ = run_render(config_dir, &stdin_json(cwd));
}

/// Polls an arbitrary JSON file (not necessarily a cache file) until it exists and
/// parses, up to `S04_POLL_TIMEOUT`.
fn poll_until_file_json(path: &Path, label: &str) -> Value {
    let start = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(path) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                return value;
            }
        }
        if start.elapsed() >= S04_POLL_TIMEOUT {
            panic!("S-04 case {label}: {path:?} did not become ready within {S04_POLL_TIMEOUT:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Polls the cache file at `config_dir`/`key` until `pred` holds, up to `S04_POLL_TIMEOUT`.
fn poll_until_cache<F: Fn(&Value) -> bool>(
    config_dir: &Path,
    key: &str,
    label: &str,
    pred: F,
) -> Value {
    let path = cache::cache_path(config_dir, key);
    let start = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                if pred(&value) {
                    return value;
                }
            }
        }
        if start.elapsed() >= S04_POLL_TIMEOUT {
            panic!(
                "S-04 case {label}: cache {key} did not reach expected state within \
                 {S04_POLL_TIMEOUT:?}"
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// REQ-03 / S-04
#[test]
fn test_s04_render_trigger_and_claim() {
    // (1) no cache: `--` and elapsed < 300ms immediately, then within 2s the cache file
    // appears claimed-and-fulfilled and `P/seen.json` proves the raw stdin reached the
    // child (stdin-forwarding).
    {
        let label = "1";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        warmup_render(d.path(), &p.as_str());
        write_render_settings(d.path(), S04_COMMAND, 5, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));

        let result = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            render_output_text(&result).contains("--"),
            "S-04 case {label}: output must show '--' immediately, got {:?}",
            result.output.stdout
        );
        assert!(
            result.elapsed < Duration::from_millis(300),
            "S-04 case {label}: elapsed {:?} must be < 300ms",
            result.elapsed
        );

        let expected_output = format!("hello-{}", basename(p.path()));
        let cache = poll_until_cache(d.path(), &key, label, |cache| {
            cache["output"] == json!(expected_output)
        });
        assert_eq!(cache["fetchedAt"], json!(NOW), "S-04 case {label} fetchedAt");
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 5_000),
            "S-04 case {label} nextFetchAt"
        );
        assert_eq!(
            cache["command"],
            json!(S04_COMMAND),
            "S-04 case {label} command"
        );
        assert_eq!(
            cache_file_mode(d.path(), &key),
            0o600,
            "S-04 case {label} cache file mode"
        );
        let seen = poll_until_file_json(&p.path().join("seen.json"), label);
        assert_eq!(
            seen["model"]["display_name"],
            json!("Probe"),
            "S-04 case {label}: raw stdin must have reached the child"
        );
    }

    // (2) fresh cache (usable, not expired): render only reads, no spawn.
    {
        let label = "2";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_render_settings(d.path(), S04_COMMAND, 5, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_cache_state(
            d.path(),
            &key,
            &CommandCache {
                fetched_at: Some(NOW - 1_000),
                next_fetch_at: NOW + 4_000,
                command: S04_COMMAND.to_owned(),
                output: Some("cached".into()),
                last_error: None,
            },
        );
        let before = read_cache_bytes(d.path(), &key);

        let result = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            render_output_text(&result).contains("cached"),
            "S-04 case {label}: output must show the cached text, got {:?}",
            result.output.stdout
        );
        assert!(
            !p.path().join("seen.json").exists(),
            "S-04 case {label}: no child must have been spawned"
        );
        assert_eq!(
            read_cache_bytes(d.path(), &key),
            before,
            "S-04 case {label}: cache bytes must be unchanged"
        );
    }

    // (3) expired nextFetchAt but usable command: render displays the stale-but-usable
    // text immediately, reclaims in the background, and the child observes the claim
    // (nextFetchAt == now+15_000) via `cache-at-run.json` while it is still running.
    {
        let label = "3";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        warmup_render(d.path(), &p.as_str());
        write_render_settings(d.path(), S04_COMMAND, 5, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_cache_state(
            d.path(),
            &key,
            &CommandCache {
                fetched_at: Some(NOW - 1_000),
                next_fetch_at: NOW - 1,
                command: S04_COMMAND.to_owned(),
                output: Some("cached".into()),
                last_error: None,
            },
        );

        let result = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            render_output_text(&result).contains("cached"),
            "S-04 case {label}: output must show the stale-but-usable text, got {:?}",
            result.output.stdout
        );
        assert!(
            result.elapsed < Duration::from_millis(300),
            "S-04 case {label}: elapsed {:?} must be < 300ms",
            result.elapsed
        );

        let cache_at_run = poll_until_file_json(&p.path().join("cache-at-run.json"), label);
        assert_eq!(
            cache_at_run["nextFetchAt"],
            json!(NOW + 15_000),
            "S-04 case {label}: cache observed mid-flight must still hold the claim value"
        );
        let expected_output = format!("hello-{}", basename(p.path()));
        poll_until_cache(d.path(), &key, label, |cache| {
            cache["output"] == json!(expected_output)
        });
    }

    // (4) claimed by someone else (fetchedAt:null, nextFetchAt in the future): render
    // must not reclaim on top of an in-flight claim.
    {
        let label = "4";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_render_settings(d.path(), S04_COMMAND, 5, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_cache_state(
            d.path(),
            &key,
            &CommandCache {
                fetched_at: None,
                next_fetch_at: NOW + 10_000,
                command: S04_COMMAND.to_owned(),
                output: None,
                last_error: None,
            },
        );
        let before = read_cache_bytes(d.path(), &key);

        let result = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            render_output_text(&result).contains("--"),
            "S-04 case {label}: output must show '--', got {:?}",
            result.output.stdout
        );
        assert!(
            !p.path().join("seen.json").exists(),
            "S-04 case {label}: no child must have been spawned over an in-flight claim"
        );
        assert_eq!(
            read_cache_bytes(d.path(), &key),
            before,
            "S-04 case {label}: cache bytes must be unchanged"
        );
    }

    // (5) rows contain no `cmd:` segment at all: no `commands/` directory is ever
    // created.
    {
        let label = "5";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_render_settings(d.path(), S04_COMMAND, 5, false);

        let _ = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            !d.path().join("commands").exists(),
            "S-04 case {label}: no commands/ directory must have been created"
        );
    }

    // (6) ttlSec:1, no cache: first render `--`; after the child has written, a second
    // render (same frozen PPULSE_NOW_MS, so now < nextFetchAt) reads the fresh output
    // without reclaiming — cache bytes identical to right after the child finished.
    {
        let label = "6";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_render_settings(d.path(), S04_COMMAND, 1, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));

        let first = run_render(d.path(), &stdin_json(&p.as_str()));
        assert!(
            render_output_text(&first).contains("--"),
            "S-04 case {label}: first render must show '--', got {:?}",
            first.output.stdout
        );

        poll_until_cache(d.path(), &key, label, |cache| {
            cache["fetchedAt"] == json!(NOW)
        });
        let bytes_after_child = read_cache_bytes(d.path(), &key);

        let second = run_render(d.path(), &stdin_json(&p.as_str()));
        let expected_output = format!("hello-{}", basename(p.path()));
        assert!(
            render_output_text(&second).contains(&expected_output),
            "S-04 case {label}: second render must show the fresh output, got {:?}",
            second.output.stdout
        );
        assert_eq!(
            read_cache_bytes(d.path(), &key),
            bytes_after_child,
            "S-04 case {label}: second render must not have reclaimed (now < nextFetchAt)"
        );
    }

    // (7) cache holds a mismatched `command` (settings changed) plus a pre-existing
    // foreign lock: render displays `--`, resets the claim to Unavailable->Claimed
    // (fetchedAt:null, nextFetchAt:now+15_000, command:<settings>, mode 0600), but the
    // spawned child cannot touch the cache because the foreign lock is still held
    // (exit 5) — bytes and lock must be unchanged 500ms later.
    {
        let label = "7";
        let d = TempDir::new(&format!("d{label}"));
        let p = TempDir::new(&format!("p{label}"));
        write_render_settings(d.path(), S04_COMMAND, 5, true);
        let key = cmd::cache_key("k8s", Some(&p.as_str()));
        seed_cache_state(
            d.path(),
            &key,
            &CommandCache {
                fetched_at: Some(NOW - 1_000),
                next_fetch_at: NOW + 4_000,
                command: "false".to_owned(),
                output: Some("stale-cmd".into()),
                last_error: None,
            },
        );
        write_lock(d.path(), &key, "other");

        let result = run_render(d.path(), &stdin_json(&p.as_str()));

        assert!(
            render_output_text(&result).contains("--"),
            "S-04 case {label}: output must show '--', got {:?}",
            result.output.stdout
        );
        let cache = read_cache_json(d.path(), &key);
        assert_eq!(cache["fetchedAt"], Value::Null, "S-04 case {label} fetchedAt");
        assert_eq!(
            cache["nextFetchAt"],
            json!(NOW + 15_000),
            "S-04 case {label} nextFetchAt"
        );
        assert_eq!(
            cache["command"],
            json!(S04_COMMAND),
            "S-04 case {label} command"
        );
        assert_eq!(cache["output"], Value::Null, "S-04 case {label} output");
        assert_eq!(
            cache_file_mode(d.path(), &key),
            0o600,
            "S-04 case {label} cache file mode"
        );
        let bytes_right_after_render = read_cache_bytes(d.path(), &key);

        thread::sleep(Duration::from_millis(500));

        assert_eq!(
            read_cache_bytes(d.path(), &key),
            bytes_right_after_render,
            "S-04 case {label}: foreign lock must have blocked the reclaim child"
        );
        assert_eq!(
            fs::read_to_string(cache::lock_path(d.path(), &key)).expect("read lock file"),
            "other",
            "S-04 case {label}: foreign lock content must be unchanged"
        );
    }
}
