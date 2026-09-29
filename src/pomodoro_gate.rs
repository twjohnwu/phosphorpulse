use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::config;
use crate::segments::pomodoro::{
    advance, format_remaining, parse_state, work_min, Phase, MAX_STATE_FILE_BYTES,
};

const MESSAGE_EN: &str = "phosphorpulse: on a break, {} left — send again after the break";
const MESSAGE_ZH: &str = "phosphorpulse：休息中，還剩 {}，休息結束後再送出";

pub fn run() -> i32 {
    // Drain stdin so the caller never sees a broken pipe.
    if !std::io::stdin().is_terminal() {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
    }
    match decide() {
        Some(message) => {
            eprintln!("{message}");
            2
        }
        None => 0,
    }
}

fn decide() -> Option<String> {
    let entrypoint = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok()?;
    if !matches!(entrypoint.as_str(), "cli" | "claude-vscode" | "claude-desktop") {
        return None;
    }
    let config = config::load_file_only().ok()?;
    let cfg = Value::Object(config.0.clone());
    if cfg["pomodoro"]["blockDuringRest"] != Value::Bool(true) {
        return None;
    }
    let path = config::settings_path()
        .parent()?
        .join("pomodoro")
        .join("shared.json");
    if std::fs::metadata(&path).ok()?.len() > MAX_STATE_FILE_BYTES {
        return None;
    }
    let state = parse_state(serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as f64;
    if now < state.phase_start_ms || now < state.last_activity_ms {
        return None;
    }
    let minutes = work_min(&cfg);
    let (state, _) = advance(state, now, false, minutes);
    if !matches!(state.phase, Phase::ShortBreak | Phase::LongBreak) {
        return None;
    }
    let remaining = state.phase_start_ms + state.phase.duration_ms(minutes)? - now;
    let template = if cfg["language"] == "zh-TW" {
        MESSAGE_ZH
    } else {
        MESSAGE_EN
    };
    Some(template.replace("{}", &format_remaining(remaining)))
}
