use super::{cache, UsageCache};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn resolve_for_render(config_dir: &Path, now: i64) -> Option<UsageCache> {
    let path = cache::cache_path(config_dir);
    let cache = cache::read_cache(&path);
    let needs_refresh = cache
        .as_ref()
        .is_none_or(|value| now >= value.next_fetch_at);

    if needs_refresh {
        let claim = UsageCache {
            fetched_at: cache.as_ref().and_then(|value| value.fetched_at),
            next_fetch_at: now.saturating_add(15_000),
            limits: cache
                .as_ref()
                .map(|value| value.limits.clone())
                .unwrap_or_default(),
        };
        if cache::write_cache(&path, &claim).is_err() {
            return cache;
        }
        let mut child = spawn_refresh();
        let cache_available = cache
            .as_ref()
            .is_some_and(|value| value.fetched_at.is_some());
        if cache_available {
            return cache;
        }
        return wait_for_cache(&path, Duration::from_millis(800), child.as_mut());
    }

    // Parsable, unexpired claim window: fetched_at absent means it's someone
    // else's in-flight claim — wait on it rather than shortcut-returning.
    if cache
        .as_ref()
        .is_some_and(|value| value.fetched_at.is_none())
    {
        return wait_for_cache(&path, Duration::from_millis(800), None);
    }
    cache
}

fn spawn_refresh() -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
    Command::new(exe)
        .arg("usage-refresh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

fn wait_for_cache(
    path: &Path,
    budget: Duration,
    mut child: Option<&mut Child>,
) -> Option<UsageCache> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(value) = cache::read_cache(path) {
            if value.fetched_at.is_some() {
                return Some(value);
            }
        }
        if let Some(handle) = child.as_deref_mut() {
            let _ = handle.try_wait();
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}
