use std::ffi::OsString;

use phosphorpulse::{config::model::Config, protocol::RenderContext};
use serde_json::{Value, json};

struct ColumnsGuard {
    columns: Option<OsString>,
}

impl ColumnsGuard {
    fn new() -> Self {
        let guard = Self {
            columns: std::env::var_os("COLUMNS"),
        };
        unsafe {
            std::env::set_var("COLUMNS", "120");
        }
        guard
    }
}

impl Drop for ColumnsGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.columns {
                Some(value) => std::env::set_var("COLUMNS", value),
                None => std::env::remove_var("COLUMNS"),
            }
        }
    }
}

fn single_segment_config(segment: &str, nerd_font: Option<bool>) -> Value {
    let mut config = Value::Object(Config::defaults().0);
    config["rows"] = json!([{"layout": "fixed", "segments": [segment]}]);
    if let Some(enabled) = nerd_font {
        config
            .as_object_mut()
            .expect("Config::defaults is an object")
            .insert("nerdFont".into(), Value::Bool(enabled));
    }
    config
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// REQ-01 / S-01: protocol parsing preserves existing values and sanitizes new fields.
#[test]
fn test_s01_protocol_parses_and_sanitizes() {
    let stdin_a = json!({
        "session_name": "ppp",
        "fast_mode": true,
        "thinking": {"enabled": false},
        "output_style": {"name": "Concise"},
        "model": {"display_name": "Opus Fixture A"},
        "context_window": {"used_percentage": 42.5},
        "rate_limits": {"five_hour": {"used_percentage": 73.25}}
    });
    let stdin_b = json!({});
    let stdin_c = json!({
        "fast_mode": "yes",
        "thinking": true,
        "output_style": "Concise",
        "session_name": "a[31mb\nc"
    });
    let stdin_d = json!({"session_name": " \t "});
    let stdin_e = json!({"session_name": " \u{202e}ab\u{200d}c "});
    let stdin_f = json!({"session_name": "a\u{2028}b\u{200e}c\u{061c}"});
    let config_x = json!({"nerdFont": true});
    let config_y = json!({"nerdFont": "yes"});
    let config_z = json!({});

    let a = RenderContext::from_value(&stdin_a, Some(&config_x));
    let b = RenderContext::from_value(&stdin_b, Some(&config_z));
    let c = RenderContext::from_value(&stdin_c, Some(&config_y));
    let d = RenderContext::from_value(&stdin_d, Some(&config_z));
    let e = RenderContext::from_value(&stdin_e, Some(&config_z));
    let f = RenderContext::from_value(&stdin_f, Some(&config_z));

    assert_eq!(
        a.session_name.as_deref(),
        Some("ppp"),
        "REQ-01 / S-01: fixture A session_name must parse"
    );
    assert_eq!(a.fast_mode, Some(true));
    assert_eq!(a.thinking_enabled, Some(false));
    assert_eq!(a.output_style.as_deref(), Some("Concise"));
    assert!(a.nerd_font);
    assert_eq!(a.model_display_name.as_deref(), Some("Opus Fixture A"));
    assert_eq!(a.context_used_percentage, Some(42.5));
    assert_eq!(a.five_hour.used_percentage, Some(73.25));

    assert_eq!(b.session_name, None);
    assert_eq!(b.fast_mode, None);
    assert_eq!(b.thinking_enabled, None);
    assert_eq!(b.output_style, None);
    assert!(!b.nerd_font);

    assert_eq!(c.fast_mode, None);
    assert_eq!(c.thinking_enabled, None);
    assert_eq!(c.output_style, None);
    assert_eq!(c.session_name.as_deref(), Some("a[31mbc"));
    assert!(!c.nerd_font);
    assert_eq!(d.session_name, None);
    assert_eq!(e.session_name.as_deref(), Some("ab\u{200d}c"));
    assert_eq!(f.session_name.as_deref(), Some("abc"));
}

/// REQ-02, REQ-03 / S-02: default states hide and nerdFont selects exact glyphs.
#[test]
fn test_s02_hide_at_default_and_glyph_switch() {
    let _columns = ColumnsGuard::new();
    let cases = [
        ("fastMode", json!({"fast_mode": false}), None, ""),
        ("fastMode", json!({"fast_mode": true}), Some(true), "\u{f0e7} fast"),
        ("fastMode", json!({"fast_mode": true}), None, "fast"),
        ("thinking", json!({"thinking": {"enabled": true}}), None, ""),
        ("thinking", json!({"thinking": {"enabled": false}}), Some(true), "\u{f05e} think"),
        ("thinking", json!({"thinking": {"enabled": false}}), Some(false), "no-think"),
        ("outputStyle", json!({"output_style": {"name": "default"}}), None, ""),
        ("outputStyle", json!({"output_style": {"name": "Concise"}}), Some(true), "\u{f1fc} Concise"),
        ("outputStyle", json!({"output_style": {"name": "Concise"}}), None, "style:Concise"),
        ("session", json!({}), None, ""),
        ("session", json!({"session_name": "ppp"}), Some(true), "\u{f02b} ppp"),
        ("session", json!({"session_name": "ppp"}), Some(false), "#ppp"),
    ];

    for (index, (segment, stdin, nerd_font, expected)) in cases.into_iter().enumerate() {
        let rendered = phosphorpulse::render::render_value(
            stdin,
            single_segment_config(segment, nerd_font),
        );
        let plain = strip_ansi(&rendered);
        assert_eq!(
            plain.trim(),
            expected,
            "REQ-02, REQ-03 / S-02 table row {} ({segment})",
            index + 1
        );
    }
}

/// REQ-02, REQ-03 / S-03: the 24-column cap preserves grapheme clusters.
#[test]
fn test_s03_width_cap_cluster_safe() {
    let _columns = ColumnsGuard::new();
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
    let cases = [
        ("a", "session", json!({"session_name": "a".repeat(23)}), None, format!("#{}", "a".repeat(23))),
        ("b", "session", json!({"session_name": "a".repeat(30)}), None, format!("#{}…", "a".repeat(22))),
        ("d", "session", json!({"session_name": "中".repeat(12)}), None, format!("#{}…", "中".repeat(11))),
        ("e", "session", json!({"session_name": family.repeat(12)}), None, format!("#{}…", family.repeat(11))),
        ("f", "outputStyle", json!({"output_style": {"name": "b".repeat(30)}}), None, format!("style:{}…", "b".repeat(17))),
        ("g", "session", json!({"session_name": "a".repeat(30)}), Some(true), format!("\u{f02b} {}…", "a".repeat(21))),
    ];

    for (label, segment, stdin, nerd_font, expected) in cases {
        let rendered = phosphorpulse::render::render_value(
            stdin,
            single_segment_config(segment, nerd_font),
        );
        let plain = strip_ansi(&rendered);
        assert_eq!(
            plain.trim(),
            expected,
            "REQ-02, REQ-03 / S-03 case ({label})"
        );
    }
}
