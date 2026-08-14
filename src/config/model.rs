use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The configuration document.  Keeping its schema open lets this loader
/// remain no stricter than the TS lite validator as fields are added there.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Config(pub Map<String, Value>);

impl Config {
    pub fn defaults() -> Self {
        let mut values = Map::new();
        values.insert("style".into(), Value::String("lean".into()));
        values.insert("activeTemplate".into(), Value::String("matrix-tron".into()));
        values.insert("colorDepth".into(), Value::String("auto".into()));
        values.insert(
            "rows".into(),
            serde_json::json!([{"layout": "auto", "segments": ["model", "ctx"]}]),
        );
        values.insert(
            "subagent".into(),
            serde_json::json!({"segments": ["name", "model", "ctx", "elapsed"]}),
        );
        values.insert(
            "gauge".into(),
            serde_json::json!({"barWidth": 20, "warnPct": 65, "hotPct": 85}),
        );
        Self(values)
    }

    /// TS liteValidate permits omitted properties, but the renderer dereferences
    /// rows.  This is the approved fail-loud exception to that permissiveness.
    pub fn validate_renderable(&self) -> Result<(), String> {
        let rows = self.0.get("rows").ok_or("/rows: required for rendering")?;
        let rows = rows.as_array().ok_or("/rows: must be an array")?;
        if rows.is_empty() {
            return Err("/rows: must have at least 1 item(s)".into());
        }
        if rows.len() > 3 {
            return Err("/rows: must have at most 3 item(s)".into());
        }
        if let Some(bar_width) = self.0.get("gauge").and_then(|gauge| gauge.get("barWidth")) {
            if !matches!(bar_width.as_f64(), Some(width) if width.fract() == 0.0 && (1.0..=80.0).contains(&width))
            {
                return Err("/gauge/barWidth: must be an integer between 1 and 80".into());
            }
        }
        if let Some(segments) = self.0.get("segments") {
            let segments = segments.as_object().ok_or("/segments: must be an object")?;
            for (id, segment) in segments {
                let segment = segment
                    .as_object()
                    .ok_or_else(|| format!("/segments/{id}: must be an object"))?;
                for key in ["fg", "bg"] {
                    if let Some(color) = segment.get(key) {
                        if !matches!(color.as_str(), Some(value) if is_six_digit_hex(value)) {
                            return Err(format!(
                                "/segments/{id}/{key}: must match pattern ^#[0-9A-Fa-f]{{6}}$"
                            ));
                        }
                    }
                }
            }
        }
        for (index, row) in rows.iter().enumerate() {
            let row = row
                .as_object()
                .ok_or_else(|| format!("/rows/{index}: must be an object"))?;
            if let Some(layout) = row.get("layout") {
                if !matches!(layout.as_str(), Some("auto" | "fixed")) {
                    return Err(format!(
                        "/rows/{index}/layout: must be one of \"auto\", \"fixed\""
                    ));
                }
            }
            if let Some(segments) = row.get("segments") {
                let segments = segments
                    .as_array()
                    .ok_or_else(|| format!("/rows/{index}/segments: must be an array"))?;
                if segments
                    .iter()
                    .any(|segment| !matches!(segment, Value::String(s) if !s.is_empty()))
                {
                    return Err(format!(
                        "/rows/{index}/segments: entries must be non-empty strings"
                    ));
                }
            }
        }
        Ok(())
    }
}

fn is_six_digit_hex(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}
