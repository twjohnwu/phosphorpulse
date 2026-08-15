use std::{fs, path::Path, process::Command, time::SystemTime};

use serde_json::{json, Map, Value};

use crate::{atomic_write, clock, protocol::RenderContext};

const MAX_STATE_FILE_BYTES: u64 = 64 * 1024;
const WORK_DEFAULT_MIN: i64 = 25;
const SHORT_BREAK_MIN: i64 = 5;
const LONG_BREAK_MIN: i64 = 15;
const ROUNDS_PER_CYCLE: i64 = 4;
const IDLE_STOP_MS: i64 = 10 * 60_000;
const NOTIFY_RATE_LIMIT_MS: i64 = 30_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Work,
    ShortBreak,
    LongBreak,
    Stopped,
}

impl Phase {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "work" => Some(Self::Work),
            "shortBreak" => Some(Self::ShortBreak),
            "longBreak" => Some(Self::LongBreak),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::ShortBreak => "shortBreak",
            Self::LongBreak => "longBreak",
            Self::Stopped => "stopped",
        }
    }
    fn duration_ms(self, work_min: f64) -> Option<f64> {
        Some(match self {
            Self::Work => work_min * 60_000.0,
            Self::ShortBreak => SHORT_BREAK_MIN as f64 * 60_000.0,
            Self::LongBreak => LONG_BREAK_MIN as f64 * 60_000.0,
            Self::Stopped => return None,
        })
    }
}

#[derive(Clone)]
struct State {
    phase: Phase,
    round: f64,
    phase_start_ms: f64,
    last_activity_ms: f64,
    sessions: Map<String, Value>,
    last_notified_at_ms: Option<f64>,
}

pub struct Pomodoro {
    pub text: String,
    pub fg: &'static str,
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn finite_nonnegative(doc: &Map<String, Value>, key: &str) -> Option<f64> {
    doc.get(key)?
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

// Deliberately mirrors the TS top-level-only validator: session entries remain opaque.
fn parse_state(value: Value) -> Option<State> {
    let doc = value.as_object()?;
    let phase = Phase::parse(doc.get("phase")?.as_str()?)?;
    let round = finite_nonnegative(doc, "round")?;
    let phase_start_ms = finite_nonnegative(doc, "phaseStartMs")?;
    let last_activity_ms = finite_nonnegative(doc, "lastActivityMs")?;
    let sessions = doc.get("sessions")?.as_object()?.clone();
    let last_notified_at_ms = match doc.get("lastNotifiedAtMs") {
        Some(value) => Some(value.as_f64().filter(|n| n.is_finite() && *n >= 0.0)?),
        None => None,
    };
    Some(State {
        phase,
        round,
        phase_start_ms,
        last_activity_ms,
        sessions,
        last_notified_at_ms,
    })
}

fn read_state(session_id: &str, state_dir: &Path) -> Option<State> {
    if !valid_session_id(session_id) {
        return None;
    }
    let path = state_dir.join("shared.json");
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() > MAX_STATE_FILE_BYTES {
        return None;
    }
    parse_state(serde_json::from_slice(&fs::read(path).ok()?).ok()?)
}

fn work_min(config: &Value) -> f64 {
    config
        .get("pomodoro")
        .and_then(|v| v.get("workMin"))
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(WORK_DEFAULT_MIN as f64)
}

fn activity_signature(ctx: &RenderContext) -> String {
    format!(
        "{}:{}",
        ctx.total_input_tokens.unwrap_or(0.0),
        ctx.total_output_tokens.unwrap_or(0.0)
    )
}

fn activity_changed(
    sessions: &mut Map<String, Value>,
    session_id: &str,
    signature: &str,
    now: f64,
) -> bool {
    let changed = sessions
        .get(session_id)
        .and_then(Value::as_object)
        .and_then(|entry| entry.get("signature"))
        .and_then(Value::as_str)
        != Some(signature);
    sessions.insert(
        session_id.into(),
        json!({ "signature": signature, "lastSeenMs": now }),
    );
    changed
}

fn advance(mut state: State, now: f64, changed: bool, work_min: f64) -> (State, bool) {
    if state.phase == Phase::Stopped {
        if changed {
            state.phase = Phase::Work;
            state.round = 1.0;
            state.phase_start_ms = now;
            state.last_activity_ms = now;
        }
        return (state, false);
    }
    if changed {
        state.last_activity_ms = now;
    }
    if now < state.phase_start_ms {
        state.phase_start_ms = now;
        return (state, false);
    }
    let mut crossings = 0;
    loop {
        let boundary =
            state.phase_start_ms + state.phase.duration_ms(work_min).expect("active phase");
        if now < boundary {
            break;
        }
        if boundary - state.last_activity_ms >= IDLE_STOP_MS as f64 {
            state.phase = Phase::Stopped;
            state.phase_start_ms = boundary;
            crossings += 1;
            break;
        }
        match state.phase {
            Phase::Work if state.round < ROUNDS_PER_CYCLE as f64 => state.phase = Phase::ShortBreak,
            Phase::Work => state.phase = Phase::LongBreak,
            Phase::ShortBreak => {
                state.phase = Phase::Work;
                state.round += 1.0;
            }
            Phase::LongBreak => {
                state.phase = Phase::Work;
                state.round = 1.0;
            }
            Phase::Stopped => unreachable!(),
        }
        state.phase_start_ms = boundary;
        crossings += 1;
        if crossings >= 10_000 {
            state.phase_start_ms = now;
            break;
        }
    }
    (state, crossings > 0)
}

fn write_state(session_id: &str, state_dir: &Path, state: &State) -> bool {
    if !valid_session_id(session_id) {
        return false;
    }
    let mut doc = Map::new();
    doc.insert("phase".into(), json!(state.phase.as_str()));
    doc.insert("round".into(), json_number(state.round));
    doc.insert("phaseStartMs".into(), json_number(state.phase_start_ms));
    doc.insert("lastActivityMs".into(), json_number(state.last_activity_ms));
    doc.insert("sessions".into(), Value::Object(state.sessions.clone()));
    if let Some(last) = state.last_notified_at_ms {
        doc.insert("lastNotifiedAtMs".into(), json_number(last));
    }
    let _ = fs::create_dir_all(state_dir);
    atomic_write::clear_stale_temps(state_dir, SystemTime::now());
    atomic_write::write_atomic(
        &state_dir.join("shared.json"),
        serde_json::to_vec(&Value::Object(doc))
            .unwrap_or_default()
            .as_slice(),
    )
    .is_ok()
}

fn json_number(number: f64) -> Value {
    if number.fract() == 0.0 && number <= i64::MAX as f64 {
        json!(number as i64)
    } else {
        json!(number)
    }
}

fn notify(phase: Phase) {
    let script = match phase {
        Phase::Work => "display notification \"Time to work! ⏱\" with title \"phosphorpulse\"",
        _ => "display notification \"Get some rest! ☕\" with title \"phosphorpulse\"",
    };
    let _ = Command::new("osascript").args(["-e", script]).spawn();
}

pub fn resolve(ctx: &RenderContext, config: &Value, config_dir: &Path) -> Pomodoro {
    let Some(session_id) = ctx.session_id.as_deref().filter(|id| valid_session_id(id)) else {
        return Pomodoro {
            text: "⏱ --:--".into(),
            fg: "text.dim",
        };
    };
    let now_ms = clock::now_ms();
    let now = now_ms as f64;
    let state_dir = config_dir.join("pomodoro");
    let previous = read_state(session_id, &state_dir);
    let mut state = previous.clone().unwrap_or_else(|| State {
        phase: Phase::Work,
        round: 1.0,
        phase_start_ms: now,
        last_activity_ms: now,
        sessions: Map::new(),
        last_notified_at_ms: None,
    });
    let changed = activity_changed(
        &mut state.sessions,
        session_id,
        &activity_signature(ctx),
        now,
    );
    let work_min = work_min(config);
    let (mut state, crossed_boundary) = if previous.is_some() {
        advance(state, now, changed, work_min)
    } else {
        (state, false)
    };
    let notify_intent = previous.as_ref().is_some_and(|old| {
        crossed_boundary
            && old.phase != Phase::Stopped
            && state.phase != Phase::Stopped
            && std::env::consts::OS == "macos"
            && state
                .last_notified_at_ms
                .is_none_or(|last| now - last >= NOTIFY_RATE_LIMIT_MS as f64)
    });
    if notify_intent {
        state.last_notified_at_ms = Some(now);
    }
    let wrote = write_state(session_id, &state_dir, &state);
    if notify_intent && wrote {
        notify(state.phase);
    }
    let Some(duration) = state.phase.duration_ms(work_min) else {
        return Pomodoro {
            text: "⏱ --:--".into(),
            fg: "text.dim",
        };
    };
    let remaining = (state.phase_start_ms + duration - now).max(0.0);
    let seconds = (remaining / 1000.0).floor() as i64;
    let text = match state.phase {
        Phase::Work => format!("⏱ {:02}:{:02}", seconds / 60, seconds % 60),
        _ => format!("☕ {:02}:{:02}", seconds / 60, seconds % 60),
    };
    let blinking = seconds <= 5 && (now_ms / 1000) % 2 != 0;
    Pomodoro {
        text,
        fg: if blinking {
            "text.dim"
        } else if state.phase == Phase::Work {
            "pomodoro.work"
        } else {
            "pomodoro.break"
        },
    }
}
