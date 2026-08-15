//! Codex configuration input/output shell.

use phosphorpulse::atomic_write::write_atomic;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

pub const KNOWN_STATUS_LINE_IDS: [&str; 8] = [
    "model-with-reasoning",
    "current-dir",
    "git-branch",
    "run-state",
    "codex-version",
    "context-used",
    "five-hour-limit",
    "weekly-limit",
];

/// The three editable `tui` values from Codex's configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexConfig {
    pub status_line: Vec<String>,
    pub status_line_use_colors: bool,
    pub theme: String,
}

/// A config value together with the bytes observed before a confirmed write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSnapshot {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub config: CodexConfig,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigIoError {
    AbortStale,
    Io(String),
    Parse(String),
}

/// Reads the three editable Codex configuration values and captures a byte snapshot.
pub fn read_config(path: &Path) -> Result<ConfigSnapshot, ConfigIoError> {
    let bytes = std::fs::read(path).map_err(io_error)?;
    let document = parse_document(&bytes)?;
    Ok(ConfigSnapshot {
        path: path.to_path_buf(),
        bytes,
        config: read_values(&document)?,
    })
}

/// Applies the three editable values if the on-disk bytes still match `snapshot`.
pub fn write_config(snapshot: &ConfigSnapshot, config: &CodexConfig) -> Result<(), ConfigIoError> {
    let current = std::fs::read(&snapshot.path).map_err(io_error)?;
    if current != snapshot.bytes {
        return Err(ConfigIoError::AbortStale);
    }

    let mut document = parse_document(&current)?;
    let tui = tui_table_mut(&mut document)?;
    tui.insert(
        "status_line",
        Item::Value(Value::Array(status_line_array(&config.status_line))),
    );
    tui.insert(
        "status_line_use_colors",
        toml_edit::value(config.status_line_use_colors),
    );
    tui.insert("theme", toml_edit::value(&config.theme));
    write_atomic(&snapshot.path, document.to_string().as_bytes()).map_err(io_error)
}

fn io_error(error: std::io::Error) -> ConfigIoError {
    ConfigIoError::Io(error.to_string())
}

fn parse_document(bytes: &[u8]) -> Result<DocumentMut, ConfigIoError> {
    std::str::from_utf8(bytes)
        .map_err(|error| ConfigIoError::Parse(error.to_string()))?
        .parse::<DocumentMut>()
        .map_err(|error| ConfigIoError::Parse(error.to_string()))
}

fn read_values(document: &DocumentMut) -> Result<CodexConfig, ConfigIoError> {
    let tui = match document.get("tui") {
        Some(item) => Some(
            item.as_table()
                .ok_or_else(|| ConfigIoError::Parse("tui must be a table".into()))?,
        ),
        None => None,
    };
    let status_line = match tui.and_then(|table| table.get("status_line")) {
        Some(item) => item
            .as_array()
            .ok_or_else(|| ConfigIoError::Parse("tui.status_line must be an array".into()))?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    ConfigIoError::Parse("tui.status_line must contain strings".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => KNOWN_STATUS_LINE_IDS.map(str::to_owned).into(),
    };
    let status_line_use_colors = match tui.and_then(|table| table.get("status_line_use_colors")) {
        Some(item) => item.as_bool().ok_or_else(|| {
            ConfigIoError::Parse("tui.status_line_use_colors must be a bool".into())
        })?,
        None => true,
    };
    let theme = match tui.and_then(|table| table.get("theme")) {
        Some(item) => item
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ConfigIoError::Parse("tui.theme must be a string stem".into()))?,
        None => String::new(),
    };
    Ok(CodexConfig {
        status_line,
        status_line_use_colors,
        theme,
    })
}

fn tui_table_mut(document: &mut DocumentMut) -> Result<&mut Table, ConfigIoError> {
    if document.get("tui").is_none() {
        document.insert("tui", Item::Table(Table::new()));
    }
    document
        .get_mut("tui")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| ConfigIoError::Parse("tui must be a table".into()))
}

fn status_line_array(ids: &[String]) -> Array {
    let mut array = Array::default();
    for id in ids {
        array.push(id.as_str());
    }
    array
}
