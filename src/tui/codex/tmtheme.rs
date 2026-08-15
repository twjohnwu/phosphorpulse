//! XML TextMate theme parsing, colour editing, and Codex scope splitting.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

use phosphorpulse::atomic_write::write_atomic;
use plist::{Dictionary, Value};

const CODEX_SCOPES: [&str; 4] = [
    "constant.numeric",
    "constant",
    "constant.language",
    "storage.type",
];

/// A parsed theme together with the bytes observed before a confirmed write.
#[derive(Clone, Debug)]
pub struct TmThemeSnapshot {
    pub path: PathBuf,
    bytes: Vec<u8>,
    pub theme: TmTheme,
}

/// The plist value tree exposed through the editor's colour-only operations.
#[derive(Clone, Debug)]
pub struct TmTheme {
    value: Value,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TmThemeError {
    AbortStale,
    Io(String),
    Parse(String),
}

/// Reads an XML plist theme. Binary and malformed plist input are deliberately
/// errors because the editor must remain read-only for those files.
pub fn read_tmtheme(path: &Path) -> Result<TmThemeSnapshot, TmThemeError> {
    let bytes = std::fs::read(path).map_err(io_error)?;
    if bytes.starts_with(b"bplist00") {
        return Err(TmThemeError::Parse(
            "binary plist tmTheme is not editable".into(),
        ));
    }
    let value = Value::from_reader_xml(Cursor::new(&bytes))
        .map_err(|error| TmThemeError::Parse(error.to_string()))?;
    validate_theme(&value)?;
    Ok(TmThemeSnapshot {
        path: path.to_path_buf(),
        bytes,
        theme: TmTheme { value },
    })
}

/// Writes the edited theme only when the on-disk bytes still match its read
/// snapshot, preventing a confirmed write from overwriting Codex changes.
pub fn write_tmtheme(snapshot: &TmThemeSnapshot) -> Result<(), TmThemeError> {
    let current = std::fs::read(&snapshot.path).map_err(io_error)?;
    if current != snapshot.bytes {
        return Err(TmThemeError::AbortStale);
    }
    let mut xml = Vec::new();
    snapshot
        .theme
        .value
        .to_writer_xml(&mut xml)
        .map_err(|error| TmThemeError::Parse(error.to_string()))?;
    write_atomic(&snapshot.path, &xml).map_err(io_error)
}

/// Appends dedicated entries for the four Codex status scopes. The old fused
/// entry stays intact; appended exact selectors win matching ties.
pub fn split_codex_scopes(theme: &mut TmTheme) -> Result<(), TmThemeError> {
    let initial_colours = CODEX_SCOPES.map(|scope| {
        theme
            .resolved_foreground(scope)
            .map(str::to_owned)
            .ok_or_else(|| TmThemeError::Parse(format!("no foreground resolves for {scope}")))
    });
    let settings = scope_settings_mut(&mut theme.value)?;
    for (scope, colour) in CODEX_SCOPES.into_iter().zip(initial_colours) {
        if has_dedicated_scope(settings, scope) {
            continue;
        }
        let mut colours = Dictionary::new();
        colours.insert("foreground".into(), Value::String(colour?));
        let mut entry = Dictionary::new();
        entry.insert("scope".into(), Value::String(scope.into()));
        entry.insert("settings".into(), Value::Dictionary(colours));
        settings.push(Value::Dictionary(entry));
    }
    Ok(())
}

impl TmTheme {
    pub fn global_foreground(&self) -> Option<&str> {
        global_colours(&self.value)?.get("foreground")?.as_string()
    }

    pub fn global_background(&self) -> Option<&str> {
        global_colours(&self.value)?.get("background")?.as_string()
    }

    /// Finds the longest comma-split selector that prefixes `scope`; later
    /// entries break equal-length ties so appended split entries take effect.
    pub fn resolved_foreground(&self, scope: &str) -> Option<&str> {
        self.resolved_scope_foreground(scope)
            .or_else(|| self.global_foreground())
    }

    /// Finds a foreground from a scoped entry only. This lets consumers try
    /// several Codex renderer scopes before deliberately falling back to the
    /// theme's global foreground.
    pub fn resolved_scope_foreground(&self, scope: &str) -> Option<&str> {
        let mut matched = None;
        let mut matched_length = 0;
        for entry in scope_settings(&self.value).ok()? {
            let Some(entry) = entry.as_dictionary() else {
                continue;
            };
            let Some(colour) = entry_colours(entry)
                .and_then(|colours| colours.get("foreground"))
                .and_then(Value::as_string)
            else {
                continue;
            };
            let Some(selectors) = entry.get("scope").and_then(Value::as_string) else {
                continue;
            };
            for selector in selectors.split(',').map(str::trim) {
                if selector_prefixes(scope, selector) && selector.len() >= matched_length {
                    matched = Some(colour);
                    matched_length = selector.len();
                }
            }
        }
        matched
    }

    pub fn set_global_foreground(&mut self, colour: &str) {
        if let Some(colours) = global_colours_mut(&mut self.value) {
            colours.insert("foreground".into(), Value::String(colour.into()));
        }
    }

    /// Updates the editable global background colour without touching other
    /// plist settings.
    pub fn set_global_background(&mut self, colour: &str) {
        if let Some(colours) = global_colours_mut(&mut self.value) {
            colours.insert("background".into(), Value::String(colour.into()));
        }
    }

    pub fn set_scope_foreground(&mut self, scope: &str, colour: &str) -> Result<(), TmThemeError> {
        let settings = scope_settings_mut(&mut self.value)?;
        let entry = settings
            .iter_mut()
            .rev()
            .find_map(|entry| {
                let entry = entry.as_dictionary_mut()?;
                (entry.get("scope").and_then(Value::as_string) == Some(scope)).then_some(entry)
            })
            .ok_or_else(|| TmThemeError::Parse(format!("missing dedicated scope {scope}")))?;
        let colours = entry
            .get_mut("settings")
            .and_then(Value::as_dictionary_mut)
            .ok_or_else(|| {
                TmThemeError::Parse(format!("scope {scope} has no settings dictionary"))
            })?;
        colours.insert("foreground".into(), Value::String(colour.into()));
        Ok(())
    }
}

fn io_error(error: std::io::Error) -> TmThemeError {
    TmThemeError::Io(error.to_string())
}

fn validate_theme(value: &Value) -> Result<(), TmThemeError> {
    scope_settings(value).map(|_| ())
}

fn root(value: &Value) -> Result<&Dictionary, TmThemeError> {
    value
        .as_dictionary()
        .ok_or_else(|| TmThemeError::Parse("tmTheme root must be a dictionary".into()))
}

fn scope_settings(value: &Value) -> Result<&Vec<Value>, TmThemeError> {
    root(value)?
        .get("settings")
        .and_then(Value::as_array)
        .ok_or_else(|| TmThemeError::Parse("tmTheme settings must be an array".into()))
}

fn scope_settings_mut(value: &mut Value) -> Result<&mut Vec<Value>, TmThemeError> {
    value
        .as_dictionary_mut()
        .and_then(|root| root.get_mut("settings"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| TmThemeError::Parse("tmTheme settings must be an array".into()))
}

fn global_colours(value: &Value) -> Option<&Dictionary> {
    scope_settings(value)
        .ok()?
        .first()?
        .as_dictionary()?
        .get("settings")?
        .as_dictionary()
}

fn global_colours_mut(value: &mut Value) -> Option<&mut Dictionary> {
    scope_settings_mut(value)
        .ok()?
        .first_mut()?
        .as_dictionary_mut()?
        .get_mut("settings")?
        .as_dictionary_mut()
}

fn entry_colours(entry: &Dictionary) -> Option<&Dictionary> {
    entry.get("settings")?.as_dictionary()
}

fn selector_prefixes(scope: &str, selector: &str) -> bool {
    !selector.is_empty()
        && (scope == selector
            || scope
                .strip_prefix(selector)
                .is_some_and(|suffix| suffix.starts_with('.')))
}

fn has_dedicated_scope(settings: &[Value], scope: &str) -> bool {
    settings.iter().any(|entry| {
        entry
            .as_dictionary()
            .and_then(|entry| entry.get("scope"))
            .and_then(Value::as_string)
            == Some(scope)
    })
}
