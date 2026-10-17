pub mod row_builder;
pub mod themes;
use crate::{
    cmd, config,
    protocol::RenderContext,
    segments::{
        cache, external, pomodoro,
        simple::{self, DIM, Segment},
    },
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use themes::Theme;
fn depth(cfg: &Value) -> &str {
    match cfg
        .get("colorDepth")
        .and_then(Value::as_str)
        .unwrap_or("auto")
    {
        "auto" => {
            if std::env::var("COLORTERM").ok().as_deref() == Some("truecolor") {
                "truecolor"
            } else if std::env::var("TERM")
                .ok()
                .is_some_and(|x| x.ends_with("-256color"))
            {
                "256color"
            } else {
                "16color"
            }
        }
        x => x,
    }
}
pub fn color(hex: &str, d: &str, bg: bool) -> String {
    let [r, g, b] = crate::jsx::color::parse_hex(hex);
    if d == "truecolor" {
        format!("\x1b[{};2;{r};{g};{b}m", if bg { 48 } else { 38 })
    } else {
        let (a, z) = crate::jsx::color::downgrade(hex);
        if d == "256color" {
            format!("\x1b[{};5;{a}m", if bg { 48 } else { 38 })
        } else {
            format!("\x1b[{}m", if bg { z + 10 } else { z })
        }
    }
}
// Main-line segments read `segments.<id>` (then the row colour); subagent
// segments read only `subagent.colors.<id>`.
fn configured_color<'a>(
    id: &str,
    row: Option<&'a Value>,
    cfg: &'a Value,
    key: &str,
    subagent: bool,
) -> Option<&'a str> {
    if subagent {
        return cfg
            .get("subagent")
            .and_then(|v| v.get("colors"))
            .and_then(|v| v.get(id))
            .and_then(|v| v.get(key))
            .and_then(Value::as_str);
    }
    cfg.get("segments")
        .and_then(|v| v.get(id))
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .or_else(|| {
            row.and_then(|value| value.get("color"))
                .and_then(|value| value.get(key))
                .and_then(Value::as_str)
        })
}
fn palette_color<'a>(value: &'a str, t: &'a Theme) -> &'a str {
    if value.starts_with('#') {
        value
    } else {
        t.palette.get(value).map(String::as_str).unwrap_or(value)
    }
}
// Gauge segments: a configured fg replaces only the normal "ok" color, so
// warn/hot/dim thresholds stay visible. `gauge.{ok,warn,hot}Color` recolor
// their state for every gauge segment (a segment fg still beats okColor).
pub fn is_gauge(id: &str) -> bool {
    matches!(id, "limit5h" | "limit7d" | "limitModel" | "ctx" | "cache")
}
fn gauge_color<'a>(cfg: &'a Value, key: &str) -> Option<&'a str> {
    cfg.get("gauge")
        .and_then(|gauge| gauge.get(key))
        .and_then(Value::as_str)
}
fn wrap(
    id: &str,
    s: Segment,
    row: Option<&Value>,
    cfg: &Value,
    t: &Theme,
    d: &str,
    subagent: bool,
) -> String {
    let fg = if id.starts_with("cmd:") && s.fg == Some("text.dim") {
        s.fg
    } else if is_gauge(id) {
        match s.fg {
            Some("ok" | "ctx.ok") => configured_color(id, row, cfg, "fg", subagent)
                .or_else(|| gauge_color(cfg, "okColor"))
                .or(s.fg),
            Some("warn") => gauge_color(cfg, "warnColor").or(s.fg),
            Some("hot") => gauge_color(cfg, "hotColor").or(s.fg),
            _ => s.fg,
        }
    } else {
        configured_color(id, row, cfg, "fg", subagent).or(s.fg)
    };
    let bg = configured_color(id, row, cfg, "bg", subagent);
    let text = s
        .text
        .replace(DIM, &color(t.palette.get("text.dim").unwrap(), d, false));
    if fg.is_none() && bg.is_none() && !s.bold {
        text
    } else {
        let mut prefix = String::new();
        if s.bold {
            prefix.push_str("\x1b[1m");
        }
        if let Some(fg) = fg {
            prefix.push_str(&color(palette_color(fg, t), d, false));
        }
        if let Some(bg) = bg {
            prefix.push_str(&color(palette_color(bg, t), d, true));
        }
        format!("{prefix}{text}\x1b[0m")
    }
}
fn segment(
    id: &str,
    c: &RenderContext,
    cmd: &cmd::CmdContext<'_>,
    e: &external::Values,
    pomodoro: Option<&pomodoro::Pomodoro>,
    usage: Option<&crate::usage::UsageCache>,
    now: i64,
) -> Option<Segment> {
    match id {
        "model" => simple::model(c),
        "effort" => simple::effort(c),
        "dir" => simple::dir(c),
        "ctx" => simple::ctx(c),
        "limit5h" => simple::limit5h(c),
        "limit7d" => simple::limit7d(c),
        "limitModel" => Some(simple::limit_model(usage, now, c)),
        "cache" => cache::segment(c, now),
        "version" => simple::version(c),
        "cost" => simple::cost(c),
        "burn" => simple::burn(c),
        "git" => e.git.as_ref().map(|x| Segment {
            text: format!("⎇ {x}"),
            fg: Some(if x.contains('*') {
                "git.dirty"
            } else {
                "git.ok"
            }),
            bold: false,
        }),
        "node" => Some(Segment {
            text: e.node.clone().unwrap_or_else(|| "node:—".into()),
            fg: Some("node"),
            bold: false,
        }),
        "python" => Some(Segment {
            text: e.python.clone().unwrap_or_else(|| "py:—".into()),
            fg: Some("python"),
            bold: false,
        }),
        "pomodoro" => pomodoro.map(|value| Segment {
            text: value.text.clone(),
            fg: Some(value.fg),
            bold: false,
        }),
        "session" => simple::session(c),
        "fastMode" => simple::fast_mode(c),
        "outputStyle" => simple::output_style(c),
        "thinking" => simple::thinking(c),
        id if id.starts_with("cmd:") => cmd::segment(&id["cmd:".len()..], c, cmd, now),
        _ => None,
    }
}
enum RenderMode {
    Live,
    Preview,
}

/// Renders a value using the existing live renderer behavior.
pub fn render_value(raw: Value, cfg: Value) -> String {
    let config_dir = config::settings_path()
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    render_value_at(raw, cfg, config_dir, RenderMode::Live, None, &[])
}

/// Renders a TUI preview without consulting or mutating live renderer state.
pub fn render_value_preview(raw: Value, cfg: Value, config_dir: PathBuf) -> String {
    render_value_preview_with_columns(raw, cfg, config_dir, None)
}

/// Renders a TUI preview at the width of its pane.  The live CLI still owns
/// terminal-width discovery; the TUI must use its already-known widget width
/// so `flex` expands inside the preview instead of an unrelated 80-column
/// fallback.
pub fn render_value_preview_at_columns(
    raw: Value,
    cfg: Value,
    config_dir: PathBuf,
    columns: usize,
) -> String {
    render_value_preview_with_columns(raw, cfg, config_dir, Some(columns))
}

fn render_value_preview_with_columns(
    raw: Value,
    cfg: Value,
    config_dir: PathBuf,
    columns: Option<usize>,
) -> String {
    render_value_at(raw, cfg, config_dir, RenderMode::Preview, columns, &[])
}

fn render_value_at(
    raw: Value,
    cfg: Value,
    config_dir: PathBuf,
    mode: RenderMode,
    columns: Option<usize>,
    raw_stdin: &[u8],
) -> String {
    let template = themes::builtin(
        cfg.get("activeTemplate")
            .and_then(Value::as_str)
            .unwrap_or("matrix-tron"),
    );
    let c = RenderContext::from_value(&raw, Some(&cfg));
    let count = cfg
        .get("rows")
        .and_then(Value::as_array)
        .map(|x| x.len())
        .unwrap_or(template.rows.len());
    let row_segments = (0..count)
        .map(|index| resolved_row_segments(&cfg, &template, index))
        .collect::<Vec<_>>();
    let now = crate::clock::now_ms();
    let cmd_ctx = cmd::CmdContext {
        config_dir: &config_dir,
        raw_stdin,
        is_preview: matches!(&mode, RenderMode::Preview),
        specs: config::commands(&cfg),
    };
    let (need_git, need_node, need_python) =
        row_segments
            .iter()
            .flatten()
            .fold((false, false, false), |needs, id| {
                (
                    needs.0 || *id == "git",
                    needs.1 || *id == "node",
                    needs.2 || *id == "python",
                )
            });
    let external = match mode {
        RenderMode::Live => external::resolve(
            &config_dir,
            c.cwd.as_deref(),
            need_git,
            need_node,
            need_python,
        ),
        RenderMode::Preview => external::Values {
            git: need_git.then(|| "preview-git".into()),
            node: need_node.then(|| "node:24.18.1".into()),
            python: need_python.then(|| "py:3.13.7".into()),
        },
    };
    let needs_pomodoro = row_segments.iter().flatten().any(|id| *id == "pomodoro");
    let pomodoro = match mode {
        RenderMode::Live => needs_pomodoro.then(|| pomodoro::resolve(&c, &cfg, &config_dir)),
        RenderMode::Preview => needs_pomodoro.then(|| pomodoro::Pomodoro {
            text: "⏱ 25:00".into(),
            fg: "text.dim",
        }),
    };
    if matches!(mode, RenderMode::Live) {
        cache::persist(&c, &config_dir, now);
    }
    let needs_usage = row_segments.iter().flatten().any(|id| *id == "limitModel");
    let usage = if needs_usage {
        match mode {
            RenderMode::Live => crate::usage::trigger::resolve_for_render(&config_dir, now),
            RenderMode::Preview => Some(crate::usage::preview_cache(now)),
        }
    } else {
        None
    };
    let cols = columns.unwrap_or_else(|| {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|x| {
                x.parse::<usize>()
                    .ok()
                    .filter(|x| *x > 0)
                    .map(|x| x.saturating_sub(4).max(20))
            })
            .unwrap_or(80)
    });
    let d = depth(&cfg);
    (0..count)
        .map(|i| {
            let row = cfg
                .get("rows")
                .and_then(Value::as_array)
                .and_then(|x| x.get(i));
            let out = row_segments[i]
                .iter()
                .copied()
                .filter_map(|id| {
                    if id == "flex" {
                        Some(row_builder::FLEX.into())
                    } else {
                        segment(
                            id,
                            &c,
                            &cmd_ctx,
                            &external,
                            pomodoro.as_ref(),
                            usage.as_ref(),
                            now,
                        )
                        .map(|s| wrap(id, s, row, &cfg, &template, d, false))
                    }
                })
                .collect::<Vec<_>>();
            row_builder::build_row(
                &out,
                row.and_then(|r| r.get("layout"))
                    .and_then(Value::as_str)
                    .unwrap_or("auto"),
                cols,
                "",
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn resolved_row_segments<'a>(
    cfg: &'a Value,
    template: &'a themes::Theme,
    index: usize,
) -> Vec<&'a str> {
    cfg.get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.get(index))
        .and_then(|row| row.get("segments"))
        .and_then(Value::as_array)
        .map(|segments| segments.iter().filter_map(Value::as_str).collect())
        .unwrap_or_else(|| {
            template
                .rows
                .get(index)
                .map(|row| row.segments.iter().map(String::as_str).collect())
                .unwrap_or_default()
        })
}
pub fn render(raw: Value, raw_stdin: Vec<u8>) {
    match config::load() {
        Ok(x) => {
            let config_dir = config::settings_path()
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            print!(
                "{}",
                render_value_at(
                    raw,
                    Value::Object(x.config.0),
                    config_dir,
                    RenderMode::Live,
                    None,
                    &raw_stdin,
                )
            );
        }
        Err(e) => println!("⚠ warning: phosphorpulse config invalid — {e}"),
    }
}
pub fn render_subagent(raw: Value, _raw_stdin: Vec<u8>) {
    let cfg = config::load()
        .map(|x| Value::Object(x.config.0))
        .unwrap_or_else(|_| Value::Object(config::default_config().0));
    print!("{}", render_subagent_value(raw, cfg));
}

/// Renders subagent JSONL without writing to stdout, for the TUI preview.
pub fn render_subagent_value(raw: Value, cfg: Value) -> String {
    let d = depth(&cfg);
    let t = themes::builtin(
        cfg.get("activeTemplate")
            .and_then(Value::as_str)
            .unwrap_or("matrix-tron"),
    );
    let c = RenderContext::from_value(&raw, Some(&cfg));
    let tasks = raw.get("tasks").and_then(Value::as_array);
    if tasks.is_none() {
        return "phosphorpulse subagent\n".into();
    }
    let ids = cfg
        .get("subagent")
        .and_then(|v| v.get("segments"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_else(|| t.subagent.iter().map(String::as_str).collect());
    let mut output = String::new();
    for task in tasks.into_iter().flatten().take(20) {
        let detail = task
            .get("description")
            .or_else(|| task.get("label"))
            .and_then(Value::as_str);
        let identity = crate::protocol::resolve_subagent_role_name(
            task,
            raw.get("transcript_path").and_then(Value::as_str),
        );
        let transcript_role = crate::protocol::transcript_role(
            task,
            raw.get("transcript_path").and_then(Value::as_str),
        );
        let name = identity.as_deref().or(detail);
        let mut parts = Vec::new();
        for id in &ids {
            let rendered = match *id {
                "name" => name.map(|x| {
                    let text = match role_icon(task, transcript_role.as_deref(), &cfg) {
                        Some(icon) => format!("{icon} {x}"),
                        None => x.to_owned(),
                    };
                    (text, "sub.name", true)
                }),
                "desc" if identity.is_some() => detail.map(|x| (x.to_owned(), "text.dim", false)),
                "model" => task
                    .get("model")
                    .and_then(Value::as_str)
                    .map(|x| (format!("◆ {}", model_short(x)), "sub.model", true)),
                "effort" => task.get("effort").and_then(Value::as_str).map(|x| {
                    (
                        format!("ψ {}", if x == "medium" { "med" } else { x }),
                        "effort",
                        false,
                    )
                }),
                "ctx" => subagent_ctx(task, &c),
                "elapsed" => subagent_elapsed(task),
                "tokenCount" => task
                    .get("tokenCount")
                    .and_then(num_value)
                    .map(|x| (simple::tokens(x), "text.dim", false)),
                "activity" => subagent_activity(task).map(|x| (x, "text.dim", false)),
                _ => None,
            };
            if let Some((text, key, bold)) = rendered {
                let s = Segment {
                    text,
                    fg: Some(key),
                    bold,
                };
                parts.push(wrap(id, s, None, &cfg, &t, d, true));
            }
        }
        let content = parts
            .into_iter()
            .map(|part| format!(" {part} "))
            .collect::<String>();
        let id = serde_json::to_string(task.get("id").unwrap_or(&Value::Null))
            .expect("serialize task id");
        let content = serde_json::to_string(&content).expect("serialize task content");
        output.push_str(&format!("{{\"id\":{id},\"content\":{content}}}\n"));
    }
    output
}

/// Glyph from `segments.name.roleIcons`, keyed by the task's own `agentType`
/// when present, otherwise by the transcript-recorded role.
fn role_icon<'a>(task: &Value, transcript_role: Option<&str>, cfg: &'a Value) -> Option<&'a str> {
    let icons = cfg
        .get("segments")?
        .get("name")?
        .get("roleIcons")?
        .as_object()?;
    let lookup = |key: &str| icons.get(key).and_then(Value::as_str);
    match task.get("agentType").and_then(Value::as_str) {
        Some(agent_type) => lookup(agent_type),
        None => transcript_role.and_then(lookup),
    }
}

/// Sparkline of the last 16 numeric `tokenSamples`, scaled from min to max.
fn subagent_activity(task: &Value) -> Option<String> {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let samples = task
        .get("tokenSamples")?
        .as_array()?
        .iter()
        .filter_map(num_value)
        .collect::<Vec<_>>();
    let samples = &samples[samples.len().saturating_sub(16)..];
    if samples.is_empty() {
        return None;
    }
    let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    Some(
        samples
            .iter()
            .map(|x| {
                let level = if range > 0. { (x - min) / range * 7. } else { 0. };
                BLOCKS[level.round() as usize]
            })
            .collect(),
    )
}

fn num_value(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        _ => None,
    }
}
fn model_short(model: &str) -> String {
    let s = model.strip_prefix("claude-").unwrap_or(model);
    let is_version = |part: &str| part.len() <= 2 && part.chars().all(|c| c.is_ascii_digit());
    let mut out = String::new();
    let mut prev_version = false;
    for part in s.split('-') {
        let version = is_version(part);
        if !out.is_empty() {
            out.push(if version && prev_version { '.' } else { ' ' });
        }
        if part.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
        prev_version = version;
    }
    out
}
fn subagent_ctx(task: &Value, c: &RenderContext) -> Option<(String, &'static str, bool)> {
    let tokens = task.get("tokenCount").and_then(num_value)?;
    let token_text = simple::tokens(tokens);
    if let Some(window) = task
        .get("contextWindowSize")
        .and_then(num_value)
        .filter(|x| *x > 0.)
    {
        let pct = ((tokens * 100. / window).floor()).min(100.);
        let fg = if pct >= c.hot_pct {
            "hot"
        } else if pct >= c.warn_pct {
            "warn"
        } else {
            "ok"
        };
        return Some((
            format!(
                "⬡ {} {}% {DIM}{token_text}",
                gauge(pct, c.gauge_width),
                pct as i64
            ),
            fg,
            false,
        ));
    }
    Some((format!("⬡ {token_text}"), "text.dim", false))
}
fn subagent_elapsed(task: &Value) -> Option<(String, &'static str, bool)> {
    let start = task.get("startTime").and_then(num_value)? as i64;
    // checked_sub: `startTime` comes from task JSON and may be extreme.
    let diff = crate::clock::now_ms().checked_sub(start).filter(|d| *d >= 0)?;
    let seconds = diff / 1000;
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    let text = if h > 0 {
        format!("⧖ {h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("⧖ {m}m{s:02}s")
    } else {
        format!("⧖ {s}s")
    };
    Some((text, "sub.elapsed", false))
}
fn gauge(p: f64, w: usize) -> String {
    let n = (p.clamp(0., 100.) * w as f64 / 100.).round() as usize;
    format!("{}{}", "▰".repeat(n), "▱".repeat(w.saturating_sub(n)))
}
