pub mod model;
pub mod overrides;

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde_json::Value;

use model::Config;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub command: String,
    pub timeout_ms: u64,
    pub ttl_sec: u64,
    pub max_width: usize,
    pub preserve_colors: bool,
}

pub fn commands(cfg: &Value) -> BTreeMap<String, CommandSpec> {
    cfg.get("commands")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| {
            let value = value.as_object()?;
            let command = value.get("command")?.as_str()?;
            Some((
                name.clone(),
                CommandSpec {
                    command: command.into(),
                    timeout_ms: value
                        .get("timeoutMs")
                        .and_then(Value::as_u64)
                        .unwrap_or(1_000),
                    ttl_sec: value.get("ttlSec").and_then(Value::as_u64).unwrap_or(5),
                    max_width: value
                        .get("maxWidth")
                        .and_then(Value::as_u64)
                        .and_then(|width| usize::try_from(width).ok())
                        .unwrap_or(24),
                    preserve_colors: value
                        .get("preserveColors")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ))
        })
        .collect()
}

pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: Config,
    pub env_paths: std::collections::BTreeSet<String>,
}

pub fn config() {
    let path = settings_path();
    let raw = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let file = raw.as_ref().and_then(Value::as_object);
    let effective = file.cloned().unwrap_or_else(|| Config::defaults().0);
    let mut document = Value::Object(effective);
    let env_paths = overrides::apply(&mut document);
    let values = document
        .as_object()
        .expect("configuration document is an object");
    println!("phosphorpulse settings file: {}", path.display());
    println!("source: default/file/env");
    for key in [
        "style",
        "activeTemplate",
        "colorDepth",
        "rows",
        "subagent",
        "gauge",
    ] {
        let source = if env_paths
            .iter()
            .any(|entry| entry == key || entry.starts_with(&format!("{key}.")))
        {
            "env"
        } else if file.is_some_and(|record| record.contains_key(key)) {
            "file"
        } else {
            "default"
        };
        let value = values.get(key).cloned().unwrap_or(Value::Null);
        println!("{key}: {value} (source: {source})");
    }
}

pub fn load() -> Result<LoadedConfig, String> {
    let path = settings_path();
    let text =
        fs::read_to_string(&path).map_err(|error| format!("cannot read config file: {error}"))?;
    let mut document: Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid JSON: {error}"))?;
    let env_paths = overrides::apply(&mut document);
    let config: Config =
        serde_json::from_value(document).map_err(|error| format!("invalid config: {error}"))?;
    config
        .validate_renderable()
        .map_err(|error| format!("invalid config: {error}"))?;
    Ok(LoadedConfig {
        path,
        config,
        env_paths,
    })
}

pub fn default_config() -> Config {
    Config::defaults()
}

pub fn settings_path() -> PathBuf {
    let directory = std::env::var_os("PPULSE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude/phosphorpulse")
        });
    directory.join("settings.json")
}
