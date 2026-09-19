//! Custom shell-command segments (`cmd:<name>`). Populated by tasks T3+.

use std::io::Read;

use serde::{Deserialize, Serialize};

pub mod ansi;
pub mod cache;
pub mod run;
pub mod trigger;

/// REQ-02's stdin-forwarding cap (Domain Language 「stdin 轉餵」): render's own reader
/// already stops at the same 1 MiB, but `cmd-refresh` enforces it independently on its
/// own stdin.
const STDIN_FORWARD_MAX_BYTES: usize = 1_048_576;

/// `render`'s detached-thread stdin forward (`trigger::spawn_refresh`) races `render`'s
/// own process exit: a spawned thread that hasn't been joined is simply killed when
/// `main` returns, so a slow-to-schedule write can lose the forwarded bytes entirely.
/// Bytes up to this cap are written synchronously (before `render` can exit) instead;
/// real payloads (Claude Code's transcript JSON) are 1-2 KB, well under it, so the
/// detached thread now only runs for the rare oversized payload. 32 KiB comfortably
/// fits a pipe's kernel buffer (64 KiB on macOS/Linux) without blocking the caller.
const STDIN_SYNC_MAX_BYTES: usize = 32 * 1024;

pub const CLAIM_WINDOW_MS: i64 = 15_000;
pub const FAILURE_BACKOFF_MS: i64 = 30_000;
pub const EXPIRED_AFTER_MS: i64 = 86_400_000;
pub const MIN_FRESH_MS: i64 = 60_000;
pub const MAX_OUTPUT_BYTES: usize = 4_096;
pub const MAX_CACHE_FILE_BYTES: u64 = 65_536;

/// Diagnostic strings stored in `CommandCache::last_error` are capped at this many bytes
/// (char-boundary safe) so a chatty command can't bloat the cache file.
const LAST_ERROR_CAP_BYTES: usize = 200;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandCache {
    pub fetched_at: Option<i64>,
    pub next_fetch_at: i64,
    pub command: String,
    pub output: Option<String>,
    /// Short diagnostic from the most recent failed run (`None` on success, and `None`/
    /// absent for cache files written before this field existed — `skip_serializing_if`
    /// keeps a successful write byte-identical to the pre-diagnosability format).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_error: Option<String>,
}

pub enum RunOutcome {
    Success(String),
    Failure(String),
}

#[derive(PartialEq, Debug)]
pub enum Freshness {
    Fresh,
    Stale,
    Expired,
}

pub struct CmdContext<'a> {
    pub config_dir: &'a std::path::Path,
    pub raw_stdin: &'a [u8],
    pub is_preview: bool,
    pub specs: std::collections::BTreeMap<String, crate::config::CommandSpec>,
}

pub fn segment(
    name: &str,
    c: &crate::protocol::RenderContext,
    cmd: &CmdContext<'_>,
    now: i64,
) -> Option<crate::segments::simple::Segment> {
    let Some(spec) = cmd.specs.get(name) else {
        return Some(unavailable_segment());
    };
    if cmd.is_preview {
        return Some(display_segment(format!("{name}: preview"), "text"));
    }

    let Some(cache) = trigger::resolve_for_render(
        cmd.config_dir,
        name,
        spec,
        c.cwd.as_deref(),
        cmd.raw_stdin,
        now,
    ) else {
        return Some(unavailable_segment());
    };
    let fg = match freshness(cache.fetched_at, now, spec.ttl_sec) {
        Freshness::Fresh => "text",
        Freshness::Stale => "text.dim",
        Freshness::Expired => return Some(unavailable_segment()),
    };
    let text = cache
        .output
        .as_deref()
        .map(|raw| display_text(raw, spec.preserve_colors, spec.max_width))
        .filter(|text| !text.is_empty());
    Some(text.map_or_else(unavailable_segment, |text| display_segment(text, fg)))
}

fn display_segment(text: String, fg: &'static str) -> crate::segments::simple::Segment {
    crate::segments::simple::Segment {
        text,
        fg: Some(fg),
        bold: false,
    }
}

fn unavailable_segment() -> crate::segments::simple::Segment {
    display_segment("--".into(), "text")
}

pub fn cache_key(name: &str, cwd: Option<&str>) -> String {
    format!("{name}-{:016x}", fnv1a64(cwd.unwrap_or("")))
}

/// Reads at most `cap` bytes from stdin, stopping at EOF or the cap, whichever comes
/// first. Read errors are ignored — `cmd-refresh` forwards whatever it managed to read.
fn read_stdin_capped(cap: usize) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8_192];
    let mut stdin = std::io::stdin();
    while buffer.len() < cap {
        match stdin.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => buffer.extend_from_slice(&chunk[..count.min(cap - buffer.len())]),
        }
    }
    buffer
}

/// REQ-02: `phosphorpulse cmd-refresh <name> <cwd>` — runs `name`'s command, schedules
/// the result into its cache file, and releases its process lock. Returns the process
/// exit code; never prints anything (Domain Language 「執行」／「排程」／「子程序鎖」).
pub fn refresh_command(name: &str, cwd: &str) -> i32 {
    let Ok(loaded) = crate::config::load() else {
        return 2;
    };
    let cfg = serde_json::Value::Object(loaded.config.0);
    let Some(spec) = crate::config::commands(&cfg).remove(name) else {
        return 2;
    };

    let config_dir = crate::config::settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let key = cache_key(name, Some(cwd));
    let path = cache::cache_path(&config_dir, &key);
    let lock_path = cache::lock_path(&config_dir, &key);
    let Some(lock) = crate::usage::lock::RefreshLock::acquire_at(&lock_path) else {
        return 5;
    };

    let stdin_bytes = read_stdin_capped(STDIN_FORWARD_MAX_BYTES);
    let outcome = if cwd.is_empty() || !std::path::Path::new(cwd).is_dir() {
        RunOutcome::Failure("invalid working directory".to_owned())
    } else {
        run::run_with_stdin(
            &spec.command,
            std::path::Path::new(cwd),
            &stdin_bytes,
            spec.timeout_ms,
        )
    };

    let now = crate::clock::now_ms();
    let old = cache::read_cache(&path);
    let code = match outcome {
        RunOutcome::Success(_) => 0,
        RunOutcome::Failure(_) => 4,
    };
    let new_cache = schedule(outcome, old.as_ref(), now, spec.ttl_sec, &spec.command);
    let _ = cache::write_cache(&path, &new_cache);
    lock.release();
    code
}

pub fn schedule(
    outcome: RunOutcome,
    old: Option<&CommandCache>,
    now: i64,
    ttl_sec: u64,
    command: &str,
) -> CommandCache {
    match outcome {
        RunOutcome::Success(output) => CommandCache {
            fetched_at: Some(now),
            next_fetch_at: now.saturating_add(seconds_to_millis(ttl_sec)),
            command: command.to_owned(),
            output: Some(output),
            last_error: None,
        },
        RunOutcome::Failure(diagnostic) => {
            let (fetched_at, output) = kept(old, command);
            CommandCache {
                fetched_at,
                next_fetch_at: now.saturating_add(FAILURE_BACKOFF_MS),
                command: command.to_owned(),
                output,
                last_error: Some(clean_diagnostic(&diagnostic)),
            }
        }
    }
}

/// Filters a raw diagnostic string the same way `protocol::clean_text` filters render
/// input (control chars and bidi/format marks), then truncates to `LAST_ERROR_CAP_BYTES`
/// on a char boundary — the diagnostic may echo a child's stderr, which is untrusted text.
fn clean_diagnostic(diagnostic: &str) -> String {
    let cleaned = crate::protocol::clean_text(Some(diagnostic)).unwrap_or_default();
    run::truncate_utf8(&cleaned, LAST_ERROR_CAP_BYTES).to_owned()
}

pub fn display_text(raw: &str, preserve_colors: bool, max_width: usize) -> String {
    let stripped = ansi::strip_escapes(raw, preserve_colors);
    let cleaned = clean_non_escape_text(&stripped);
    let text = cleaned.trim();

    if crate::jsx::width::display_width(text) > max_width {
        let mut truncated =
            crate::render::row_builder::truncate_to_width(text, max_width.saturating_sub(1));
        truncated.push('…');
        truncated
    } else {
        text.to_owned()
    }
}

pub fn freshness(fetched_at: Option<i64>, now: i64, ttl_sec: u64) -> Freshness {
    let Some(fetched_at) = fetched_at else {
        return Freshness::Expired;
    };
    let age = now.saturating_sub(fetched_at);
    if age < 0 || age > EXPIRED_AFTER_MS {
        Freshness::Expired
    } else {
        let fresh_for = seconds_to_millis(ttl_sec.saturating_mul(2)).max(MIN_FRESH_MS);
        if age <= fresh_for {
            Freshness::Fresh
        } else {
            Freshness::Stale
        }
    }
}

fn fnv1a64(value: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    value.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

fn seconds_to_millis(seconds: u64) -> i64 {
    i64::try_from(seconds.saturating_mul(1_000)).unwrap_or(i64::MAX)
}

fn kept(old: Option<&CommandCache>, command: &str) -> (Option<i64>, Option<String>) {
    old.filter(|cache| cache.command == command)
        .map(|cache| (cache.fetched_at, cache.output.clone()))
        .unwrap_or((None, None))
}

fn clean_non_escape_text(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut cleaned = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"\x1b[") {
            let mut end = index + 2;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'm' {
                cleaned.push_str(&input[index..=end]);
                index = end + 1;
                continue;
            }
        }

        let character = input[index..].chars().next().expect("valid UTF-8 suffix");
        if !character.is_control()
            && !matches!(
                character,
                '\u{2028}'
                    | '\u{2029}'
                    | '\u{061C}'
                    | '\u{200E}'
                    | '\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2066}'..='\u{2069}'
            )
        {
            cleaned.push(character);
        }
        index += character.len_utf8();
    }

    cleaned
}
