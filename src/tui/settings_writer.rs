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
pub fn maybe_rewrite_claude_settings(_draft: &Config) -> io::Result<()> {
    let Some(home) = std::env::var_os("HOME") else {
        return Ok(());
    };
    let path = claude_settings_path(home);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut settings = parse_settings(&path, &raw)?;
    let current_refresh = settings
        .get("statusLine")
        .and_then(Value::as_object)
        .and_then(|status_line| status_line.get("refreshInterval"));
    if current_refresh == Some(&json!(1)) {
        return Ok(());
    }

    merge_statusline_blocks(&mut settings);
    backup_then_write(&path, &raw, &settings)
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
