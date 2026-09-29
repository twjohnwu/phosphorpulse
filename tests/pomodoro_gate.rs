use phosphorpulse::config::default_config;
use phosphorpulse::segments::pomodoro::{format_remaining, project};
use serde_json::{json, Value};

const T: f64 = 1_790_000_000_000.0;

fn min(m: f64) -> f64 {
    m * 60_000.0
}

/// REQ-01 / S-01
#[test]
fn test_s01_block_during_rest_validation() {
    let cases = [
        ("A", json!({}), true),
        ("B", json!({"blockDuringRest": true}), true),
        ("C", json!({"blockDuringRest": false}), true),
        ("D", json!({"blockDuringRest": "true"}), false),
        ("E", json!({"blockDuringRest": 1}), false),
    ];
    for (name, pomodoro, ok) in cases {
        let mut config = default_config();
        config.0.insert("pomodoro".into(), pomodoro);
        let result = config.validate_renderable();
        if ok {
            assert!(result.is_ok(), "case {name}: {result:?}");
        } else {
            assert_eq!(
                result,
                Err("/pomodoro/blockDuringRest: must be a boolean".to_string()),
                "case {name}"
            );
        }
    }
}

fn state(phase: &str, round: f64, start: f64, activity: f64) -> Value {
    json!({
        "phase": phase,
        "round": round,
        "phaseStartMs": T + min(start),
        "lastActivityMs": T + min(activity),
        "sessions": {},
    })
}

/// REQ-02 / S-02
#[test]
fn test_s02_advance_projection_and_format() {
    // (row, state, now offset min, phase, round, rest remaining ms)
    let rows = [
        ("A", state("work", 1.0, 0.0, 24.0), 26.0, "shortBreak", 1.0, Some(240_000.0)),
        ("B", state("longBreak", 4.0, 0.0, -1.0), 16.0, "stopped", 4.0, None),
        ("C", state("work", 4.0, 0.0, 24.0), 26.0, "longBreak", 4.0, Some(840_000.0)),
        ("D", state("stopped", 1.0, 0.0, 0.0), 60.0, "stopped", 1.0, None),
        ("E", state("shortBreak", 1.0, 0.0, -1.0), 6.0, "work", 2.0, None),
        ("F", state("longBreak", 4.0, 0.0, 10.0), 16.0, "work", 1.0, None),
        ("G", state("work", 1.0, 0.0, 10.0), 26.0, "stopped", 1.0, None),
        ("H", state("shortBreak", 1.0, 0.0, -9.0), 6.0, "stopped", 1.0, None),
    ];
    for (name, doc, now, phase, round, rest) in rows {
        let p = project(doc, T + min(now), 25.0).unwrap_or_else(|| panic!("row {name}: None"));
        assert_eq!(p.phase, phase, "row {name} phase");
        assert_eq!(p.round, round, "row {name} round");
        assert_eq!(p.rest_remaining_ms, rest, "row {name} rest");
    }
    assert_eq!(format_remaining(240_000.0), "04:00");
    assert_eq!(format_remaining(239_001.0), "04:00");
    assert_eq!(format_remaining(1.0), "00:01");
    assert_eq!(format_remaining(0.0), "00:00");
}
