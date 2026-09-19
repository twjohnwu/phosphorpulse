//! `cmd-refresh`'s command-execution variant, relocated out of
//! `segments/external.rs` (that module's guard test forbids `/bin/kill` appearing there —
//! `tests/review_fixes.rs::test_timeout_kill_is_cross_platform`). Zero behavior change from
//! the original; the unix-only pieces (`process_group`, `libc::killpg`) are
//! `#[cfg(unix)]`-gated with a `child.kill()` fallback for other platforms.

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

/// `cmd-refresh`'s stderr read cap: stderr is captured only to build a short failure
/// diagnostic (`RunOutcome::Failure`'s payload), never displayed in the segment itself,
/// so a small cap keeps a chatty/broken command from blocking the read or bloating the
/// diagnostic string.
const STDERR_CAP_BYTES: usize = 4 * 1024;

/// Kills the whole process group of `child` via `libc::killpg` (pgid = `child.id()`,
/// set by `process_group(0)` at spawn time), falling back to `child.kill()` (direct
/// child only) if `killpg` itself reports failure. Only acts while the shared handle is
/// still `Some`, mirroring the timeout guard above so a reused pid is never mis-killed.
/// Non-unix platforms have no process group to target, so they always fall back to
/// `child.kill()`.
///
/// Was previously implemented by shelling out to `/bin/kill -9 -<pgid>` and checking only
/// whether that command could be *spawned*, not its exit status. On Linux CI
/// (`ubuntu-latest`, procps-ng 4.0.4's `kill`), that invocation spawns fine but exits
/// nonzero — `kill -9 -<pgid>` without a `--` separator fails to parse the negative pgid
/// as an argument (`kill: failed to parse argument: '(null)'`), so the group was never
/// actually killed and the swallowed nonzero exit made the caller believe otherwise.
/// macOS's BSD `kill` parses the same invocation without complaint, which is why this was
/// never observed there. `killpg` sidesteps CLI argument parsing entirely and its
/// success/failure is checked directly.
fn kill_process_group(child: &Arc<Mutex<Option<std::process::Child>>>) {
    if let Some(child) = child.lock().expect("child handle poisoned").as_mut() {
        #[cfg(unix)]
        {
            let pgid = child.id();
            // SAFETY: `pgid` is a real process/group id obtained from `child.id()`;
            // `killpg` takes no pointers and cannot invalidate Rust-side state.
            let killed = unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGKILL) };
            if killed != 0 {
                let _ = child.kill();
            }
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
    }
}

/// Reads `reader` in chunks up to `cap` cumulative bytes. Hitting the cap before EOF
/// kills the process group immediately (the child may otherwise block forever writing
/// into a full pipe) and reports `too_large = true`. Shared by stdout (`STDOUT_CAP_BYTES`)
/// and stderr (`STDERR_CAP_BYTES`).
fn read_capped(
    mut reader: impl Read,
    cap: usize,
    child: &Arc<Mutex<Option<std::process::Child>>>,
) -> (Vec<u8>, bool) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8_192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return (buffer, false),
            Ok(count) => {
                buffer.extend_from_slice(&chunk[..count]);
                if buffer.len() >= cap {
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
/// boundary so the result stays valid UTF-8. `pub(crate)` so `mod.rs` can reuse it to cap
/// `CommandCache::last_error`.
pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
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
/// detached thread, reads stdout with the `STDOUT_CAP_BYTES` cap above, reads stderr
/// with the smaller `STDERR_CAP_BYTES` cap (kept only to build a `RunOutcome::Failure`
/// diagnostic, never displayed), and enforces `timeout_ms` by killing the whole process
/// group. Mirrors `command_with_timeout`'s worker-thread/mpsc/`Arc<Mutex<Option<Child>>>`
/// shape but does not reuse it: a non-empty stdout is required regardless of exit status.
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
        .stderr(Stdio::piped());
    #[cfg(unix)]
    builder.process_group(0);

    type Finished = Result<(std::process::ExitStatus, Vec<u8>, bool, Vec<u8>), std::io::Error>;
    let (finished, finished_receiver) = mpsc::channel::<Finished>();
    let child = match builder.spawn() {
        Ok(child) => Arc::new(Mutex::new(Some(child))),
        Err(error) => return crate::cmd::RunOutcome::Failure(error.to_string()),
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
    let stderr = child
        .lock()
        .expect("child handle poisoned")
        .as_mut()
        .expect("child handle missing")
        .stderr
        .take()
        .expect("piped stderr missing");

    let stdin_bytes = stdin_bytes.to_vec();
    thread::spawn(move || write_stdin_bytes(stdin, &stdin_bytes));

    let stdout_child = Arc::clone(&child);
    let stdout_reader = thread::spawn(move || read_capped(stdout, STDOUT_CAP_BYTES, &stdout_child));

    let stderr_child = Arc::clone(&child);
    let stderr_reader = thread::spawn(move || read_capped(stderr, STDERR_CAP_BYTES, &stderr_child));

    let worker_child = Arc::clone(&child);
    let worker = thread::spawn(move || {
        let status = loop {
            let exited = {
                let mut child = worker_child.lock().expect("child handle poisoned");
                child.as_mut().expect("child handle missing").try_wait()
            };
            match exited {
                // Wait on the still-shared handle (`as_mut`, not `take`) so the timeout
                // branch's `kill_process_group` keeps seeing `Some` — and can therefore
                // still kill by pgid — for as long as the readers below might block on a
                // grandchild that outlived the shell and kept a pipe open. Only after both
                // readers have returned do we take-and-drop the handle, once nothing else
                // needs it.
                Ok(Some(_)) => {
                    let mut child = worker_child.lock().expect("child handle poisoned");
                    break child.as_mut().expect("child handle missing").wait();
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => break Err(error),
            }
        };
        let (bytes, too_large) = stdout_reader.join().unwrap_or_else(|_| (Vec::new(), false));
        let (stderr_bytes, _) = stderr_reader.join().unwrap_or_else(|_| (Vec::new(), false));
        // Readers are done with the pipes; take-and-drop now. If the timeout branch races
        // in after this point, `kill_process_group` finds `None` and no-ops — but by then
        // there is nothing left to kill: the readers already observed EOF.
        let _ = worker_child.lock().expect("child handle poisoned").take();
        let _ = finished.send(status.map(|status| (status, bytes, too_large, stderr_bytes)));
    });

    let outcome = match finished_receiver.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            let _ = worker.join();
            return crate::cmd::RunOutcome::Failure(error.to_string());
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = worker.join();
            return crate::cmd::RunOutcome::Failure("internal error: worker disconnected".to_owned());
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_process_group(&child);
            drop(finished_receiver);
            return crate::cmd::RunOutcome::Failure(format!("timeout after {timeout_ms}ms"));
        }
    };
    let _ = worker.join();

    let (status, bytes, too_large, stderr_bytes) = outcome;
    if too_large {
        return crate::cmd::RunOutcome::Failure("output exceeded 65536 bytes".to_owned());
    }
    if !status.success() {
        let code = status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        let stderr_text = String::from_utf8_lossy(&stderr_bytes);
        let first_line_of_stderr = stderr_text.lines().next().unwrap_or("");
        return crate::cmd::RunOutcome::Failure(format!("exit {code}: {first_line_of_stderr}"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let first_line = text.lines().next().unwrap_or("");
    let truncated = truncate_utf8(first_line, crate::cmd::MAX_OUTPUT_BYTES);
    if truncated.is_empty() {
        crate::cmd::RunOutcome::Failure("empty output".to_owned())
    } else {
        crate::cmd::RunOutcome::Success(truncated.to_owned())
    }
}
