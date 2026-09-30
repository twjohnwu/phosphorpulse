use std::io::{IsTerminal, Read};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::config;
use crate::segments::pomodoro::{
    advance, format_remaining, parse_state, work_min, Phase, MAX_STATE_FILE_BYTES,
};

const MESSAGE_EN: &str = "phosphorpulse: on a break, {} left — send again after the break";
const MESSAGE_ZH: &str = "phosphorpulse：休息中，還剩 {}，休息結束後再送出";

const MAX_STDIN_BYTES: u64 = 8_388_608;
const NOTIFICATION_PREFIXES: [&str; 2] = ["<task-notification>", "<agent-message from="];

pub fn run() -> i32 {
    if allows_without_decision(read_stdin()) {
        return 0;
    }
    match decide() {
        Some(message) => {
            eprintln!("{message}");
            2
        }
        None => 0,
    }
}

/// Streams stdin to EOF so the caller never sees a broken pipe, keeping at
/// most `MAX_STDIN_BYTES`. `None` means undeterminable: a TTY, a read error,
/// or more than the cap.
fn read_stdin() -> Option<Vec<u8>> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut lock = stdin.lock();
    let mut buf = Vec::new();
    let read = (&mut lock).take(MAX_STDIN_BYTES + 1).read_to_end(&mut buf);
    let drained = std::io::copy(&mut lock, &mut std::io::sink());
    if read.is_err() || drained.is_err() || buf.len() as u64 > MAX_STDIN_BYTES {
        return None;
    }
    Some(buf)
}

/// True when the gate must allow without consulting the rest state: a system
/// notification prompt, or input that cannot be classified (fail-open).
fn allows_without_decision(input: Option<Vec<u8>>) -> bool {
    let Some(bytes) = input else { return true };
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(&bytes) else {
        return true;
    };
    match object.get("prompt").and_then(Value::as_str) {
        Some(prompt) => {
            let prompt = prompt.trim_start();
            NOTIFICATION_PREFIXES
                .iter()
                .any(|prefix| prompt.starts_with(prefix))
        }
        None => true,
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
