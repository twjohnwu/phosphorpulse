//! `cmd-refresh`'s command-execution variant, relocated out of
//! `segments/external.rs` (that module's guard test forbids `/bin/kill` appearing there —
//! `tests/review_fixes.rs::test_timeout_kill_is_cross_platform`). Zero behavior change from
//! the original; the unix-only pieces (`process_group`, `/bin/kill`) are `#[cfg(unix)]`-gated
//! with a `child.kill()` fallback for other platforms.

use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// `cmd-refresh`'s stdout read cap (Domain Language 「執行」): hitting this many
/// cumulative bytes before EOF is itself a failure, distinct from
/// `cmd::MAX_CACHE_FILE_BYTES` (a cache *file* read cap).
const STDOUT_CAP_BYTES: usize = 65_536;

/// Kills the whole process group of `child` via `/bin/kill -9 -<pgid>` (absolute path,
/// no PATH dependency), falling back to `child.kill()` (direct child only) if `/bin/kill`
/// itself cannot be spawned. Only acts while the shared handle is still `Some`, mirroring
/// the timeout guard above so a reused pid is never mis-killed. Non-unix platforms have no
/// process group to target, so they always fall back to `child.kill()`.
fn kill_process_group(child: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Some(child) = child.lock().expect("child handle poisoned").as_mut() {
        #[cfg(unix)]
        {
            let pgid = child.id();
            if Command::new("/bin/kill")
                .args(["-9", &format!("-{pgid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_err()
            {
                let _ = child.kill();
            }
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
    }
}

/// Reads `stdout` in chunks up to `STDOUT_CAP_BYTES` cumulative bytes. Hitting the cap
/// before EOF kills the process group immediately (the child may otherwise block forever
/// writing into a full pipe) and reports `too_large = true`.
fn read_capped_stdout(
    mut stdout: impl Read,
    child: &Arc<Mutex<Option<std::process::Child>>>,
) -> (Vec<u8>, bool) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8_192];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => return (buffer, false),
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                if buffer.len() >= STDOUT_CAP_BYTES {
                    kill_process_group(child);
                    return (buffer, true);
                }
            }
            Err(_) => return (buffer, false),
        }
    }
}

/// Writes `stdin_bytes` into `stdin` then drops it, closing the pipe. Any write error
/// (including a `BrokenPipe`/EPIPE from a command that never reads stdin, e.g. `date`) is
/// intentionally ignored — a user command choosing not to read stdin is not a failure.
fn write_stdin_bytes(mut stdin: impl Write, stdin_bytes: &[u8]) {
    let _ = stdin.write_all(stdin_bytes);
}

/// Truncates `s` to at most `max_bytes` bytes, backing off to the nearest earlier char
/// boundary so the result stays valid UTF-8.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// REQ-02's `cmd-refresh` execution variant (Domain Language 「執行」／「stdin 轉餵」):
/// runs `sh -c <command>` in `cwd`, forwards `stdin_bytes` to the child's stdin in a
/// detached thread, reads stdout with the `STDOUT_CAP_BYTES` cap above, and enforces
/// `timeout_ms` by killing the whole process group. Mirrors `command_with_timeout`'s
/// worker-thread/mpsc/`Arc<Mutex<Option<Child>>>` shape but does not reuse it: this
/// variant never falls back to stderr, and a non-empty stdout is required regardless of
/// exit status.
pub(crate) fn run_with_stdin(
    command: &str,
    cwd: &Path,
    stdin_bytes: &[u8],
    timeout_ms: u64,
) -> crate::cmd::RunOutcome {
    let mut builder = Command::new("sh");
    builder
        .args(["-c", command])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    builder.process_group(0);

    let (finished, finished_receiver) =
        mpsc::channel::<Result<(std::process::ExitStatus, Vec<u8>, bool), std::io::Error>>();
    let child = match builder.spawn() {
        Ok(child) => Arc::new(Mutex::new(Some(child))),
        Err(_) => return crate::cmd::RunOutcome::Failure,
    };
    let stdin = child
        .lock()
        .expect("child handle poisoned")
        .as_mut()
        .expect("child handle missing")
        .stdin
        .take()
        .expect("piped stdin missing");
    let stdout = child
        .lock()
        .expect("child handle poisoned")
        .as_mut()
        .expect("child handle missing")
        .stdout
        .take()
        .expect("piped stdout missing");

    let stdin_bytes = stdin_bytes.to_vec();
    thread::spawn(move || write_stdin_bytes(stdin, &stdin_bytes));

    let stdout_child = Arc::clone(&child);
    let stdout_reader = thread::spawn(move || read_capped_stdout(stdout, &stdout_child));

    let worker_child = Arc::clone(&child);
    let worker = thread::spawn(move || {
        let status = loop {
            let exited = {
                let mut child = worker_child.lock().expect("child handle poisoned");
                child.as_mut().expect("child handle missing").try_wait()
            };
            match exited {
                Ok(Some(_)) => {
                    let mut child = worker_child
                        .lock()
                        .expect("child handle poisoned")
                        .take()
                        .expect("child handle missing");
                    break child.wait();
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => break Err(error),
            }
        };
        let (bytes, too_large) = stdout_reader.join().unwrap_or_else(|_| (Vec::new(), false));
        let _ = finished.send(status.map(|status| (status, bytes, too_large)));
    });

    let outcome = match finished_receiver.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = worker.join();
            return crate::cmd::RunOutcome::Failure;
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_process_group(&child);
            drop(finished_receiver);
            return crate::cmd::RunOutcome::Failure;
        }
    };
    let _ = worker.join();

    let (status, bytes, too_large) = outcome;
    if too_large || !status.success() {
        return crate::cmd::RunOutcome::Failure;
    }
    let text = String::from_utf8_lossy(&bytes);
    let first_line = text.lines().next().unwrap_or("");
    let truncated = truncate_utf8(first_line, crate::cmd::MAX_OUTPUT_BYTES);
    if truncated.is_empty() {
        crate::cmd::RunOutcome::Failure
    } else {
        crate::cmd::RunOutcome::Success(truncated.to_owned())
    }
}
