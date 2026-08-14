pub mod model;
pub mod overrides;

use std::{fs, path::PathBuf};

use serde_json::Value;

use model::Config;

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
