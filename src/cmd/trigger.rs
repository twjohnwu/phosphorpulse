use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
};

use crate::config::CommandSpec;

use super::{
    CLAIM_WINDOW_MS, CommandCache, STDIN_FORWARD_MAX_BYTES, STDIN_SYNC_MAX_BYTES, cache, cache_key,
};

pub fn resolve_for_render(
    config_dir: &Path,
    name: &str,
    spec: &CommandSpec,
    cwd: Option<&str>,
    raw_stdin: &[u8],
    now: i64,
) -> Option<CommandCache> {
    let path = cache::cache_path(config_dir, &cache_key(name, cwd));
    let cached = cache::read_cache(&path);
    let mismatch = cached
        .as_ref()
        .is_some_and(|cache| cache.command != spec.command);
    let usable = cached.as_ref().is_some_and(|cache| {
        !mismatch && cache.fetched_at.is_some()
    });
    let others_claim = cached.as_ref().is_some_and(|cache| {
        !mismatch && cache.fetched_at.is_none() && now < cache.next_fetch_at
    });
    let needs_refresh = !others_claim
        && (cached.is_none()
            || mismatch
            || cached.as_ref().is_some_and(|cache| now >= cache.next_fetch_at));

    if needs_refresh {
        let claim = build_claim(cached.as_ref(), usable, spec, now);
        if cache::write_cache(&path, &claim).is_ok() {
            spawn_refresh(name, cwd, raw_stdin);
        }
    }

    if usable { cached } else { None }
}

fn build_claim(
    old: Option<&CommandCache>,
    usable: bool,
    spec: &CommandSpec,
    now: i64,
) -> CommandCache {
    if usable {
        let old = old.expect("usable implies a cache was read");
        CommandCache {
            next_fetch_at: now.saturating_add(CLAIM_WINDOW_MS),
            command: spec.command.clone(),
            ..old.clone()
        }
    } else {
        CommandCache {
            fetched_at: None,
            next_fetch_at: now.saturating_add(CLAIM_WINDOW_MS),
            command: spec.command.clone(),
            output: None,
            last_error: None,
        }
    }
}

fn spawn_refresh(name: &str, cwd: Option<&str>, raw_stdin: &[u8]) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(mut child) = Command::new(exe)
        .args(["cmd-refresh", name, cwd.unwrap_or("")])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let Some(mut stdin) = child.stdin.take() else {
        return;
    };
    let forwarded = &raw_stdin[..raw_stdin.len().min(STDIN_FORWARD_MAX_BYTES)];
    // Write the bytes that fit within `STDIN_SYNC_MAX_BYTES` synchronously, before
    // `render` (our caller) can exit and kill this thread mid-write (a detached thread
    // dropped by an exiting `main` is not joined, so a lost race here used to hand the
    // child an empty stdin). Real payloads are 1-2 KB, so this covers them without ever
    // spawning a thread; only a payload that exceeds the sync cap still needs one for the
    // remainder.
    let (sync_part, rest) = forwarded.split_at(forwarded.len().min(STDIN_SYNC_MAX_BYTES));
    let _ = stdin.write_all(sync_part);
    if rest.is_empty() {
        return;
    }
    let rest = rest.to_vec();
    thread::spawn(move || {
        let _ = stdin.write_all(&rest);
    });
}
