//! RED coverage for the custom-command configuration schema.

use phosphorpulse::{
    config::{self, CommandSpec, model::Config},
    render::themes,
};
use serde_json::{Value, json};

fn settings_with_commands(commands: Value) -> Value {
    let mut settings = Value::Object(Config::defaults().0);
    settings
        .as_object_mut()
        .expect("Config::defaults() must be an object")
        .insert("commands".into(), commands);
    settings
}

fn validate(settings: &Value) -> Result<(), String> {
    serde_json::from_value::<Config>(settings.clone())
        .expect("settings JSON must deserialize as Config")
        .validate_renderable()
}

fn assert_invalid(settings: &Value, expected: &str) {
    let error = validate(settings).expect_err("settings must be rejected");
    assert!(
        error.contains(expected),
        "expected error containing {expected:?}, got {error:?}"
    );
}

/// REQ-01 / S-01: named commands validate and receive documented defaults.
#[test]
fn test_s01_commands_schema() {
    let case_a = settings_with_commands(json!({
        "k8s": {"command": "kubectl config current-context"}
    }));
    assert!(validate(&case_a).is_ok());
    assert_eq!(
        config::commands(&case_a)["k8s"],
        CommandSpec {
            command: "kubectl config current-context".into(),
            timeout_ms: 1_000,
            ttl_sec: 5,
            max_width: 24,
            preserve_colors: false,
        }
    );

    let case_b = settings_with_commands(json!({
        "bad name": {"command": "x"}
    }));
    assert_invalid(
        &case_b,
        "/commands/bad name: name must match [A-Za-z0-9_-]{1,32}",
    );

    let case_c = settings_with_commands(json!({
        "k8s": {"command": ""}
    }));
    assert_invalid(
        &case_c,
        "/commands/k8s/command: must be a non-empty string",
    );

    let case_d = settings_with_commands(json!({
        "k8s": {"command": "x", "timeoutMs": 50}
    }));
    assert_invalid(
        &case_d,
        "/commands/k8s/timeoutMs: must be >= 100, got 50",
    );

    let case_e = settings_with_commands(json!({
        "k8s": {"command": "x", "ttlSec": 0}
    }));
    assert_invalid(
        &case_e,
        "/commands/k8s/ttlSec: must be >= 1, got 0",
    );

    let case_f = settings_with_commands(json!({
        "k8s": {"command": "x", "maxWidth": 7}
    }));
    assert_invalid(
        &case_f,
        "/commands/k8s/maxWidth: must be >= 8, got 7",
    );

    let case_g = settings_with_commands(json!({
        "k8s": {"command": "x", "preserveColors": "yes"}
    }));
    assert_invalid(
        &case_g,
        "/commands/k8s/preserveColors: must be a boolean",
    );

    let case_h = settings_with_commands(json!({"k8s": "echo"}));
    assert_invalid(&case_h, "/commands/k8s: must be an object");

    let mut case_i = Value::Object(Config::defaults().0);
    case_i
        .as_object_mut()
        .expect("Config::defaults() must be an object")
        .insert(
            "rows".into(),
            json!([{"layout": "auto", "segments": ["model", "cmd:nope"]}]),
        );
    assert!(validate(&case_i).is_ok());

    let case_j = settings_with_commands(json!({
        "k8s": {"command": "x".repeat(4_097)}
    }));
    assert_invalid(
        &case_j,
        "/commands/k8s/command: must be a non-empty string (<= 4096 bytes)",
    );

    let case_k = settings_with_commands(json!({
        "k8s": {"command": "x", "timeoutMs": 1.5}
    }));
    assert_invalid(
        &case_k,
        "/commands/k8s/timeoutMs: must be an integer",
    );

    let defaults = Value::Object(Config::defaults().0);
    assert!(config::commands(&defaults).is_empty());

    for theme_name in themes::names() {
        let mut theme_settings = Value::Object(Config::defaults().0);
        theme_settings
            .as_object_mut()
            .expect("Config::defaults() must be an object")
            .insert("activeTemplate".into(), json!(theme_name));
        assert!(
            config::commands(&theme_settings).is_empty(),
            "built-in theme {theme_name} must not declare commands"
        );
    }
}
