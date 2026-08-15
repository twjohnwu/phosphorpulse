//! Template memento persistence for the TUI.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use phosphorpulse::atomic_write::write_atomic;
use phosphorpulse::render::themes;
use serde_json::Value;

const NAME_PATTERN: &str = "[A-Za-z0-9_-]+";

fn is_builtin_template_name(name: &str) -> bool {
    themes::names().contains(&name)
}

/// Validates names which designate a user-owned template file. Built-in
/// templates are code constants and therefore cannot be overwritten or
/// imported over.
fn validate_name(name: &str) -> io::Result<()> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        if is_builtin_template_name(name) {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid template name \"{name}\": built-in templates are read-only"),
            ))
        } else {
            Ok(())
        }
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid template name \"{name}\": name must match /^{NAME_PATTERN}$/"),
        ))
    }
}

fn template_path(name: &str, templates_dir: &Path) -> io::Result<PathBuf> {
    validate_name(name)?;
    Ok(templates_dir.join(format!("{name}.json")))
}

fn json_bytes(value: &Value) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(io::Error::other)
}

fn read_json(path: &Path) -> io::Result<Value> {
    let contents = fs::read(path).map_err(|error| {
        io::Error::new(error.kind(), format!("Unable to read template source {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&contents).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unable to parse template source {}: {error}", path.display()),
        )
    })
}

/// Returns a fresh snapshot of a read-only built-in template.
///
/// The layout and palette are derived from the renderer's built-in registry;
/// the remaining fields are the common template snapshot shape shared by all
/// built-ins.
pub fn builtin_template(name: &str) -> Option<Value> {
    if !is_builtin_template_name(name) {
        return None;
    }

    let theme = themes::builtin(name);
    let rows = theme
        .rows
        .into_iter()
        .map(|row| serde_json::json!({"layout": row.layout, "segments": row.segments}))
        .collect::<Vec<_>>();
    Some(serde_json::json!({
        "rows": rows,
        "subagent": {"segments": theme.subagent},
        "gauge": {"barWidth": 20, "warnPct": 65, "hotPct": 85},
        "segments": {"dir": {"pathDepth": 99}},
        "palette": theme.palette,
    }))
}

/// Selects the template-only portion of a configuration draft.
///
/// This mirrors `draftToSnapshot` in the frozen TUI: only these five fields
/// are persisted, with its same fallbacks for absent fields.
pub fn draft_to_snapshot(draft: &Value) -> Value {
    let shape = draft.as_object();
    let field = |name: &str| shape.and_then(|object| object.get(name)).cloned();

    serde_json::json!({
        "rows": field("rows").filter(Value::is_array).unwrap_or_else(|| Value::Array(Vec::new())),
        "subagent": field("subagent").unwrap_or_else(|| serde_json::json!({"segments": []})),
        "gauge": field("gauge").unwrap_or_else(|| serde_json::json!({"barWidth": 20, "warnPct": 65, "hotPct": 85})),
        "segments": field("segments").unwrap_or_else(|| serde_json::json!({})),
        "palette": field("palette").unwrap_or_else(|| serde_json::json!({})),
    })
}

/// Saves a complete template snapshot under `templates_dir`.
pub fn save_template(name: &str, draft: &Value, templates_dir: &Path) -> io::Result<()> {
    write_atomic(&template_path(name, templates_dir)?, &json_bytes(draft)?)
}

/// Deletes a user-owned template snapshot. Built-in names are rejected.
pub fn delete_template(name: &str, templates_dir: &Path) -> io::Result<()> {
    fs::remove_file(template_path(name, templates_dir)?)
}

/// Loads a complete template snapshot by name from `templates_dir`.
pub fn load_template(name: &str, templates_dir: &Path) -> io::Result<Value> {
    if let Some(template) = builtin_template(name) {
        return Ok(template);
    }
    read_json(&template_path(name, templates_dir)?)
}

/// Exports a saved template by name to `destination`.
pub fn export_template(name: &str, destination: &Path, templates_dir: &Path) -> io::Result<()> {
    let contents = if let Some(template) = builtin_template(name) {
        json_bytes(&template)?
    } else {
        let source = template_path(name, templates_dir)?;
        fs::read(&source).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Unable to read template source {}: {error}", source.display()),
            )
        })?
    };
    write_atomic(&expand_home_path(destination), &contents)
}

/// Expands the portable leading `~/` shorthand accepted by the TUI. Keeping
/// this beside filesystem operations prevents literal tilde paths from
/// reaching the atomic writer through another export caller.
pub fn expand_home_path(path: &Path) -> PathBuf {
    path.to_str()
        .and_then(|value| value.strip_prefix("~/"))
        .and_then(|rest| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)))
        .unwrap_or_else(|| path.to_path_buf())
}

fn validation_error(errors: Vec<String>) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Template import validation failed: {}", errors.join("; ")),
    )
}

fn object<'a>(
    value: &'a Value,
    path: &str,
    errors: &mut Vec<String>,
) -> Option<&'a serde_json::Map<String, Value>> {
    value.as_object().or_else(|| {
        errors.push(format!("instancePath \"{path}\" must be object"));
        None
    })
}

fn only_fields(
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed: &[&str],
    errors: &mut Vec<String>,
) {
    for key in object.keys().filter(|key| !allowed.contains(&key.as_str())) {
        errors.push(format!(
            "instancePath \"{path}\" must NOT have additional properties ({key})"
        ));
    }
}

fn required_fields(
    object: &serde_json::Map<String, Value>,
    path: &str,
    required: &[&str],
    errors: &mut Vec<String>,
) {
    for key in required.iter().filter(|key| !object.contains_key(**key)) {
        errors.push(format!(
            "instancePath \"{path}\" must have required property '{key}'"
        ));
    }
}

fn string_array(value: Option<&Value>, path: &str, errors: &mut Vec<String>) {
    match value.and_then(Value::as_array) {
        Some(values) => {
            for (index, value) in values.iter().enumerate() {
                if !value.is_string() {
                    errors.push(format!("instancePath \"{path}/{index}\" must be string"));
                }
            }
        }
        None => errors.push(format!("instancePath \"{path}\" must be array")),
    }
}

fn validate_imported_template(template: &Value) -> io::Result<()> {
    let mut errors = Vec::new();
    let Some(root) = object(template, "(root)", &mut errors) else {
        return Err(validation_error(errors));
    };
    only_fields(
        root,
        "(root)",
        &["rows", "subagent", "gauge", "segments", "palette"],
        &mut errors,
    );
    required_fields(
        root,
        "(root)",
        &["rows", "subagent", "gauge", "segments", "palette"],
        &mut errors,
    );

    match root.get("rows").and_then(Value::as_array) {
        Some(rows) => {
            for (index, row) in rows.iter().enumerate() {
                let path = format!("/rows/{index}");
                if let Some(row) = object(row, &path, &mut errors) {
                    only_fields(row, &path, &["layout", "segments", "color"], &mut errors);
                    required_fields(row, &path, &["layout", "segments"], &mut errors);
                    if !matches!(
                        row.get("layout").and_then(Value::as_str),
                        Some("auto" | "fixed")
                    ) {
                        errors.push(format!("instancePath \"{path}/layout\" must be equal to one of the allowed values"));
                    }
                    string_array(
                        row.get("segments"),
                        &format!("{path}/segments"),
                        &mut errors,
                    );
                    if let Some(color) = row.get("color") {
                        if let Some(color) = object(color, &format!("{path}/color"), &mut errors) {
                            only_fields(
                                color,
                                &format!("{path}/color"),
                                &["fg", "bg"],
                                &mut errors,
                            );
                            for key in ["fg", "bg"] {
                                if color.get(key).is_some_and(|value| !value.is_string()) {
                                    errors.push(format!(
                                        "instancePath \"{path}/color/{key}\" must be string"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        None => errors.push("instancePath \"/rows\" must be array".into()),
    }

    if let Some(subagent) = root
        .get("subagent")
        .and_then(|value| object(value, "/subagent", &mut errors))
    {
        only_fields(subagent, "/subagent", &["segments"], &mut errors);
        required_fields(subagent, "/subagent", &["segments"], &mut errors);
        string_array(subagent.get("segments"), "/subagent/segments", &mut errors);
    }
    if let Some(gauge) = root
        .get("gauge")
        .and_then(|value| object(value, "/gauge", &mut errors))
    {
        only_fields(
            gauge,
            "/gauge",
            &["barWidth", "warnPct", "hotPct"],
            &mut errors,
        );
        required_fields(
            gauge,
            "/gauge",
            &["barWidth", "warnPct", "hotPct"],
            &mut errors,
        );
        for key in ["barWidth", "warnPct", "hotPct"] {
            if gauge.get(key).is_some_and(|value| !value.is_number()) {
                errors.push(format!("instancePath \"/gauge/{key}\" must be number"));
            }
        }
    }
    if let Some(segments) = root
        .get("segments")
        .and_then(|value| object(value, "/segments", &mut errors))
    {
        for (name, segment) in segments {
            let path = format!("/segments/{name}");
            if let Some(segment) = object(segment, &path, &mut errors) {
                only_fields(
                    segment,
                    &path,
                    &["fg", "bg", "bold", "pathDepth"],
                    &mut errors,
                );
                for key in ["fg", "bg"] {
                    if segment.get(key).is_some_and(|value| !value.is_string()) {
                        errors.push(format!("instancePath \"{path}/{key}\" must be string"));
                    }
                }
                if segment.get("bold").is_some_and(|value| !value.is_boolean()) {
                    errors.push(format!("instancePath \"{path}/bold\" must be boolean"));
                }
                if segment
                    .get("pathDepth")
                    .is_some_and(|value| !value.is_number())
                {
                    errors.push(format!("instancePath \"{path}/pathDepth\" must be number"));
                }
            }
        }
    }
    if let Some(palette) = root
        .get("palette")
        .and_then(|value| object(value, "/palette", &mut errors))
    {
        for (name, color) in palette {
            if !color.is_string() {
                errors.push(format!("instancePath \"/palette/{name}\" must be string"));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(validation_error(errors))
    }
}

/// Imports a schema-valid JSON template as `new_name` and returns its complete snapshot.
pub fn import_template(source: &Path, new_name: &str, templates_dir: &Path) -> io::Result<Value> {
    validate_name(new_name)?;
    let snapshot = read_json(source)?;
    validate_imported_template(&snapshot)?;
    save_template(new_name, &snapshot, templates_dir)?;
    Ok(snapshot)
}
