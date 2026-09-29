//! Settings persistence shell for the TUI.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use phosphorpulse::{atomic_write::write_atomic, config::model::Config};
use serde_json::{Map, Value, json};

const RENDER_COMMAND: &str = "phosphorpulse render";
const SUBAGENT_RENDER_COMMAND: &str = "phosphorpulse render --subagent";
const GATE_COMMAND: &str = "phosphorpulse pomodoro-gate";
const GATE_TIMEOUT_SEC: u64 = 5;

/// Returns the command from an object-shaped Claude settings block.
///
/// This is shared by the install screen and startup routing so both recognize
/// the same `statusLine: { command: "..." }` shape.
pub fn configured_command(settings: &Map<String, Value>, key: &str) -> Option<String> {
    let block = settings.get(key)?.as_object()?;
    block
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Upserts the statusline blocks in Claude Code's user settings file.
pub fn write_statusline_blocks(_refresh_interval: Option<u64>) -> io::Result<()> {
    let path = claude_settings_path(required_home()?);
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let mut settings = parse_settings(&path, &raw)?;
            merge_statusline_blocks(&mut settings);
            backup_then_write(&path, &raw, &settings)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut settings = Map::new();
            replace_statusline_blocks(&mut settings);
            write_settings(&path, &settings)
        }
        Err(error) => Err(error),
    }
}

/// Builds the SettingsInstall confirmation view without changing the file.
pub fn statusline_blocks_diff(_refresh_interval: Option<u64>) -> io::Result<String> {
    let path = claude_settings_path(required_home()?);
    let mut settings = match fs::read_to_string(&path) {
        Ok(raw) => parse_settings(&path, &raw)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error),
    };
    let old_status_line = settings.get("statusLine").cloned().unwrap_or(Value::Null);
    let old_subagent_status_line = settings
        .get("subagentStatusLine")
        .cloned()
        .unwrap_or(Value::Null);
    merge_statusline_blocks(&mut settings);
    let new_status_line = settings.get("statusLine").expect("statusLine was inserted");
    let new_subagent_status_line = settings
        .get("subagentStatusLine")
        .expect("subagentStatusLine was inserted");

    Ok(format!(
        "Pending settings.json changes:\n\nstatusLine:\n  {}\n  → {}\n\nsubagentStatusLine:\n  {}\n  → {}",
        old_status_line, new_status_line, old_subagent_status_line, new_subagent_status_line
    ))
}

/// Applies SaveExit's conditional Claude Code settings rewrite.
pub fn maybe_rewrite_claude_settings(draft: &Config) -> io::Result<()> {
    match plan_claude_settings(draft)? {
        Some(pending) => commit_claude_settings(pending),
        None => Ok(()),
    }
}

/// A planned SaveExit rewrite of Claude settings; `original` is `None` when the file did not exist.
pub struct PendingSettings {
    path: PathBuf,
    original: Option<String>,
    next: Map<String, Value>,
}

/// Reads, merges and syncs the gate hook in memory without writing.
pub fn plan_claude_settings(draft: &Config) -> io::Result<Option<PendingSettings>> {
    let enabled = draft
        .0
        .get("pomodoro")
        .and_then(|pomodoro| pomodoro.get("blockDuringRest"))
        == Some(&Value::Bool(true));
    let Some(home) = std::env::var_os("HOME") else {
        return if enabled {
            Err(io::Error::other("HOME is not set"))
        } else {
            Ok(None)
        };
    };
    let path = claude_settings_path(home);
    let (original, mut settings) = match fs::read_to_string(&path) {
        Ok(raw) => {
            let settings = parse_settings(&path, &raw)?;
            (Some(raw), settings)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !enabled {
                return Ok(None);
            }
            (None, Map::new())
        }
        Err(error) => return Err(error),
    };
    let need_status = original.is_some()
        && settings
            .get("statusLine")
            .and_then(Value::as_object)
            .and_then(|status_line| status_line.get("refreshInterval"))
            != Some(&json!(1));
    let need_hook = sync_gate_hook(&mut settings, enabled)?;
    if !need_status && !need_hook {
        return Ok(None);
    }
    if need_status {
        merge_statusline_blocks(&mut settings);
    }
    Ok(Some(PendingSettings {
        path,
        original,
        next: settings,
    }))
}

/// Re-reads the file, refuses if it changed since planning, then backs up and writes.
pub fn commit_claude_settings(pending: PendingSettings) -> io::Result<()> {
    let PendingSettings {
        path,
        original,
        next,
    } = pending;
    let current = match fs::read_to_string(&path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current != original {
        return Err(io::Error::other("settings.json changed during save"));
    }
    match original {
        Some(raw) => backup_then_write(&path, &raw, &next),
        None => write_settings(&path, &next),
    }
}

/// The standard-form matcher group for our `UserPromptSubmit` hook.
fn gate_group() -> Value {
    json!({"hooks": [{"type": "command", "command": GATE_COMMAND, "timeout": GATE_TIMEOUT_SEC}]})
}

fn is_own_hook(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("command")
        && item.get("command").and_then(Value::as_str) == Some(GATE_COMMAND)
}

/// Our hook entries in well-shaped parts of `hooks.UserPromptSubmit`; malformed nodes count as none.
fn own_hooks(settings: &Map<String, Value>) -> Vec<&Value> {
    settings
        .get("hooks")
        .and_then(Value::as_object)
        .and_then(|hooks| hooks.get("UserPromptSubmit"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter(|item| is_own_hook(item))
        .collect()
}

/// Returns the `UserPromptSubmit` array (created if missing), erroring on any malformed node.
fn checked_prompt_hooks(settings: &mut Map<String, Value>) -> io::Result<&mut Vec<Value>> {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| io::Error::other("settings.json: hooks is not an object"))?;
    let groups = hooks
        .entry("UserPromptSubmit")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| io::Error::other("settings.json: hooks.UserPromptSubmit is not an array"))?;
    for group in groups.iter() {
        let group = group.as_object().ok_or_else(|| {
            io::Error::other("settings.json: a hooks.UserPromptSubmit group is not an object")
        })?;
        if group.get("hooks").is_some_and(|inner| !inner.is_array()) {
            return Err(io::Error::other(
                "settings.json: a hooks.UserPromptSubmit group's hooks is not an array",
            ));
        }
    }
    Ok(groups)
}

/// Removes our entries, then any group they emptied.
fn strip_own_hooks(groups: &mut Vec<Value>) {
    groups.retain_mut(|group| {
        let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = inner.len();
        inner.retain(|item| !is_own_hook(item));
        !(inner.is_empty() && before > 0)
    });
}

/// Syncs our hook entry with `enabled`; returns whether `settings` changed.
fn sync_gate_hook(settings: &mut Map<String, Value>, enabled: bool) -> io::Result<bool> {
    let own = own_hooks(settings);
    let standard = &gate_group()["hooks"][0];
    if enabled && own.len() == 1 && own[0] == standard {
        return Ok(false);
    }
    if !enabled && own.is_empty() {
        return Ok(false);
    }
    // Work on a copy so an Err leaves `settings` untouched.
    let mut scratch = settings.clone();
    let groups = checked_prompt_hooks(&mut scratch)?;
    strip_own_hooks(groups);
    if enabled {
        groups.push(gate_group());
    } else {
        if groups.is_empty() {
            if let Some(hooks) = scratch.get_mut("hooks").and_then(Value::as_object_mut) {
                hooks.remove("UserPromptSubmit");
            }
        }
        if scratch
            .get("hooks")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
        {
            scratch.remove("hooks");
        }
    }
    *settings = scratch;
    Ok(true)
}

fn required_home() -> io::Result<std::ffi::OsString> {
    std::env::var_os("HOME")
        .ok_or_else(|| io::Error::other("writeStatuslineBlocks: HOME is not set"))
}

fn claude_settings_path(home: std::ffi::OsString) -> PathBuf {
    PathBuf::from(home).join(".claude/settings.json")
}

fn parse_settings(path: &Path, raw: &str) -> io::Result<Map<String, Value>> {
    serde_json::from_str::<Value>(raw)
        .map_err(io::Error::other)?
        .as_object()
        .cloned()
        .ok_or_else(|| {
            io::Error::other(format!(
                "settings file at {} is not an object",
                path.display()
            ))
        })
}

fn replace_statusline_blocks(settings: &mut Map<String, Value>) {
    let mut status_line = Map::new();
    status_line.insert("type".into(), json!("command"));
    status_line.insert("command".into(), json!(RENDER_COMMAND));
    status_line.insert("refreshInterval".into(), json!(1));
    settings.insert("statusLine".into(), Value::Object(status_line));
    settings.insert(
        "subagentStatusLine".into(),
        json!({"type": "command", "command": SUBAGENT_RENDER_COMMAND}),
    );
}

fn merge_statusline_blocks(settings: &mut Map<String, Value>) {
    let mut status_line = settings
        .get("statusLine")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    status_line.insert("type".into(), json!("command"));
    status_line.insert("command".into(), json!(RENDER_COMMAND));
    status_line.insert("refreshInterval".into(), json!(1));
    settings.insert("statusLine".into(), Value::Object(status_line));
    settings.insert(
        "subagentStatusLine".into(),
        json!({"type": "command", "command": SUBAGENT_RENDER_COMMAND}),
    );
}

fn backup_then_write(path: &Path, raw: &str, settings: &Map<String, Value>) -> io::Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let backup = path.with_file_name(format!("settings.json.bak-{timestamp}"));
    fs::write(backup, raw)?;
    write_settings(path, settings)
}

fn write_settings(path: &Path, settings: &Map<String, Value>) -> io::Result<()> {
    let contents = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
    write_atomic(path, &contents)
}
