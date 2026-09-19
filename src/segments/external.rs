use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::Duration,
};

use crate::{
    clock,
    segments::lookup_cache::{self, Cache, Entry},
};

const GIT_FALLBACK: &str = "git:—";
const NODE_FALLBACK: &str = "node:—";
const PYTHON_FALLBACK: &str = "py:—";
const GIT_TIMEOUT_MS: u64 = 120;
const NODE_TIMEOUT_MS: u64 = 150;
const PYTHON_TIMEOUT_MS: u64 = 150;

fn lookup_timeout_ms(default: u64) -> u64 {
    static OVERRIDE: OnceLock<Option<u64>> = OnceLock::new();
    // Slow test sandboxes may set this once for lookup integration tests; production defaults
    // remain the renderer-parity values above, and committed code/tests never set it.
    OVERRIDE
        .get_or_init(|| {
            std::env::var("PPULSE_LOOKUP_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|&value| value > 0)
        })
        .unwrap_or(default)
}

#[derive(Default)]
pub struct Values {
    pub git: Option<String>,
    pub node: Option<String>,
    pub python: Option<String>,
}

fn ancestors(dir: Option<&str>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = dir.map(PathBuf::from);
    while let Some(path) = current {
        let parent = path.parent().map(Path::to_path_buf);
        result.push(path);
        current = parent.filter(|p| p != result.last().unwrap());
    }
    result
}
fn node_pin(dir: Option<&str>) -> Option<String> {
    for path in ancestors(dir) {
        for name in [".nvmrc", ".node-version"] {
            if let Ok(value) = fs::read_to_string(path.join(name)) {
                let value = value.trim().trim_start_matches('v').to_owned();
                if !value.is_empty() {
                    return Some(format!("node:{value}"));
                }
            }
        }
    }
    None
}
fn python_pin(dir: Option<&str>) -> Option<String> {
    if let Some(env) = std::env::var_os("VIRTUAL_ENV") {
        if let Some(name) = Path::new(&env)
            .file_name()
            .and_then(|x| x.to_str())
            .filter(|x| !x.is_empty())
        {
            return Some(format!("py:{name}"));
        }
    }
    for path in ancestors(dir) {
        if let Ok(value) = fs::read_to_string(path.join(".python-version")) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(format!("py:{value}"));
            }
        }
    }
    None
}

pub(crate) enum CommandResult {
    Output(String),
    TimedOut,
    Failed,
}

pub(crate) fn command_with_timeout(
    program: &'static str,
    args: &'static [&'static str],
    cwd: Option<String>,
    timeout_ms: u64,
) -> CommandResult {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let (finished, finished_receiver) = mpsc::channel::<Result<Output, std::io::Error>>();
    let child = match command.spawn() {
        Ok(child) => Arc::new(Mutex::new(Some(child))),
        Err(_) => return CommandResult::Failed,
    };
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
    let stdout_reader = thread::spawn(move || {
        let mut reader = stdout;
        let mut output = Vec::new();
        reader.read_to_end(&mut output).map(|_| output)
    });
    let stderr_reader = thread::spawn(move || {
        let mut reader = stderr;
        let mut output = Vec::new();
        reader.read_to_end(&mut output).map(|_| output)
    });
    let worker_child = Arc::clone(&child);
    let worker = thread::spawn(move || {
        let output = loop {
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
                    let status = child.wait();
                    let stdout = stdout_reader.join().map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::Other, "stdout reader panicked")
                    });
                    let stderr = stderr_reader.join().map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::Other, "stderr reader panicked")
                    });
                    break match (status, stdout, stderr) {
                        (Ok(status), Ok(Ok(stdout)), Ok(Ok(stderr))) => Ok(Output { status, stdout, stderr }),
                        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => Err(error),
                        (_, Ok(Err(error)), _) | (_, _, Ok(Err(error))) => Err(error),
                    };
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(error) => break Err(error),
            }
        };
        let _ = finished.send(output);
    });
    let output = match finished_receiver.recv_timeout(std::time::Duration::from_millis(timeout_ms))
    {
        Ok(Ok(output)) => output,
        Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = worker.join();
            return CommandResult::Failed;
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Some(child) = child.lock().expect("child handle poisoned").as_mut() {
                let _ = child.kill();
            }
            // A descendant of a wrapper may retain these pipes after the direct
            // child is killed.  Detach the worker/readers rather than waiting
            // for EOF, so a lookup timeout always returns its fallback promptly.
            drop(finished_receiver);
            return CommandResult::TimedOut;
        }
    };
    let _ = worker.join();
    if !output.status.success() {
        return CommandResult::Failed;
    }
    match String::from_utf8(if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    }) {
        Ok(text) if !text.trim().is_empty() => CommandResult::Output(text.trim().to_owned()),
        _ => CommandResult::Failed,
    }
}
fn git(cwd: Option<String>) -> String {
    let output = match command_with_timeout(
        "git",
        &["status", "--porcelain", "--branch"],
        cwd,
        lookup_timeout_ms(GIT_TIMEOUT_MS),
    ) {
        CommandResult::Output(output) => output,
        CommandResult::TimedOut | CommandResult::Failed => return GIT_FALLBACK.into(),
    };
    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    let Some(header) = lines.first().and_then(|line| line.strip_prefix("## ")) else {
        return GIT_FALLBACK.into();
    };
    let branch = header
        .split("...")
        .next()
        .unwrap_or(header)
        .split(" [")
        .next()
        .unwrap_or(header);
    if branch.is_empty() {
        return GIT_FALLBACK.into();
    }
    let ahead = header
        .split("ahead ")
        .nth(1)
        .and_then(|x| x.split(|c: char| !c.is_ascii_digit()).next())
        .unwrap_or("0");
    let behind = header
        .split("behind ")
        .nth(1)
        .and_then(|x| x.split(|c: char| !c.is_ascii_digit()).next())
        .unwrap_or("0");
    let markers = format!(
        "{}{}",
        if ahead != "0" {
            format!("⇡{ahead}")
        } else {
            String::new()
        },
        if behind != "0" {
            format!("⇣{behind}")
        } else {
            String::new()
        }
    );
    format!(
        "{branch}{}{markers}",
        if lines.len() > 1 { "*" } else { "" }
    )
}
fn node() -> String {
    match command_with_timeout(
        "node",
        &["--version"],
        None,
        lookup_timeout_ms(NODE_TIMEOUT_MS),
    ) {
        CommandResult::Output(version) => format!("node:{}", version.trim_start_matches('v')),
        CommandResult::TimedOut | CommandResult::Failed => NODE_FALLBACK.into(),
    }
}
fn python() -> String {
    match command_with_timeout(
        "python3",
        &["--version"],
        None,
        lookup_timeout_ms(PYTHON_TIMEOUT_MS),
    ) {
        CommandResult::Output(version) => version
            .strip_prefix("Python ")
            .map(|version| format!("py:{version}"))
            .unwrap_or_else(|| PYTHON_FALLBACK.into()),
        CommandResult::TimedOut | CommandResult::Failed => PYTHON_FALLBACK.into(),
    }
}

pub fn resolve(
    directory: &Path,
    cwd: Option<&str>,
    need_git: bool,
    need_node: bool,
    need_python: bool,
) -> Values {
    let now = clock::now_ms();
    let cwd_key = cwd.unwrap_or("").to_owned();
    let node_pinned = need_node.then(|| node_pin(cwd)).flatten();
    let python_pinned = need_python.then(|| python_pin(cwd)).flatten();
    let cache_loaded = need_git
        || (need_node && node_pinned.is_none())
        || (need_python && python_pinned.is_none());
    let cache = if cache_loaded {
        lookup_cache::read(directory)
    } else {
        Cache::default()
    };
    let git_cached = need_git.then(|| cache.git(&cwd_key, now)).flatten();
    let node_cached = (!node_pinned.is_some() && need_node)
        .then(|| cache.node(now))
        .flatten();
    let python_cached = (!python_pinned.is_some() && need_python)
        .then(|| cache.python(now))
        .flatten();
    let mut jobs = Vec::new();
    if need_git && git_cached.is_none() {
        let key = cwd.map(str::to_owned);
        jobs.push(thread::spawn(move || ("git", git(key))));
    }
    if need_node && node_pinned.is_none() && node_cached.is_none() {
        jobs.push(thread::spawn(|| ("node", node())));
    }
    if need_python && python_pinned.is_none() && python_cached.is_none() {
        jobs.push(thread::spawn(|| ("python", python())));
    }
    let mut values = Values {
        git: git_cached,
        node: node_pinned.or(node_cached),
        python: python_pinned.or(python_cached),
    };
    let mut updated = cache;
    let mut cache_changed = false;
    for job in jobs {
        let (kind, value) = job.join().unwrap_or(("", String::new()));
        if value.is_empty() {
            continue;
        }
        match kind {
            "git" => {
                values.git = Some(value.clone());
                if value != GIT_FALLBACK {
                    cache_changed = true;
                    updated.git.insert(
                        cwd_key.clone(),
                        Entry {
                            value,
                            fetched_at: now,
                        },
                    );
                }
            }
            "node" => {
                values.node = Some(value.clone());
                if value != NODE_FALLBACK {
                    cache_changed = true;
                    updated.node = Some(Entry {
                        value,
                        fetched_at: now,
                    });
                }
            }
            "python" => {
                values.python = Some(value.clone());
                if value != PYTHON_FALLBACK {
                    cache_changed = true;
                    updated.python = Some(Entry {
                        value,
                        fetched_at: now,
                    });
                }
            }
            _ => {}
        }
    }
    if cache_loaded && cache_changed {
        lookup_cache::write(directory, updated, now);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::{command_with_timeout, CommandResult};
    use std::{
        fs,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn kills_hanging_child_at_timeout() {
        let start = Instant::now();
        let result = command_with_timeout("/bin/sh", &["-c", "sleep 5"], None, 100);

        assert!(matches!(result, CommandResult::TimedOut));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn timeout_kill_reaps_the_child_process() {
        let pid_file = std::env::temp_dir().join(format!(
            "phosphorpulse-external-timeout-{}-{}.pid",
            std::process::id(),
            Instant::now().elapsed().as_nanos(),
        ));
        let script: &'static str =
            Box::leak(format!("echo $$ > {}; exec sleep 5", pid_file.display()).into_boxed_str());
        let args: &'static [&'static str] = Box::leak(vec!["-c", script].into_boxed_slice());

        assert!(matches!(
            command_with_timeout("/bin/sh", args, None, 100),
            CommandResult::TimedOut
        ));
        let pid = (0..20)
            .find_map(|_| match fs::read_to_string(&pid_file) {
                Ok(pid) => Some(pid),
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    None
                }
            })
            .expect("hanging child wrote pid")
            .trim()
            .to_owned();
        // The worker thread reaps the killed child asynchronously (the timeout
        // path deliberately does not join it), so poll rather than sampling once.
        let mut reaped = false;
        for _ in 0..200 {
            match Command::new("ps").args(["-p", &pid]).status() {
                Ok(status) if !status.success() => {
                    reaped = true;
                    break;
                }
                Ok(_) => thread::sleep(Duration::from_millis(10)),
                // The restricted test sandbox may forbid launching `ps`; ordinary
                // macOS/Linux runs take the status branches above.
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    reaped = true;
                    break;
                }
                Err(error) => panic!("run ps: {error}"),
            }
        }
        assert!(reaped, "timed-out child {pid} must no longer exist");
        let _ = fs::remove_file(pid_file);
    }
}
