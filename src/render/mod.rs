pub mod row_builder;
pub mod themes;
use crate::{
    config,
    protocol::RenderContext,
    segments::{
        external, pomodoro,
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
fn configured_color<'a>(
    id: &str,
    row: Option<&'a Value>,
    cfg: &'a Value,
    key: &str,
) -> Option<&'a str> {
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
fn wrap(id: &str, s: Segment, row: Option<&Value>, cfg: &Value, t: &Theme, d: &str) -> String {
    let fg = configured_color(id, row, cfg, "fg").or(s.fg);
    let bg = configured_color(id, row, cfg, "bg");
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
    e: &external::Values,
    pomodoro: Option<&pomodoro::Pomodoro>,
) -> Option<Segment> {
    match id {
        "model" => simple::model(c),
        "effort" => simple::effort(c),
        "dir" => simple::dir(c),
        "ctx" => simple::ctx(c),
        "limit5h" => simple::limit5h(c),
        "limit7d" => simple::limit7d(c),
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
    render_value_at(raw, cfg, config_dir, RenderMode::Live, None)
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
    render_value_at(raw, cfg, config_dir, RenderMode::Preview, columns)
}

fn render_value_at(
    raw: Value,
    cfg: Value,
    config_dir: PathBuf,
    mode: RenderMode,
    columns: Option<usize>,
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
            node: need_node.then(|| "preview-node".into()),
            python: need_python.then(|| "preview-python".into()),
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
                        segment(id, &c, &external, pomodoro.as_ref())
                            .map(|s| wrap(id, s, row, &cfg, &template, d))
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
pub fn render(raw: Value) {
    match config::load() {
        Ok(x) => print!("{}", render_value(raw, Value::Object(x.config.0))),
        Err(e) => println!("⚠ warning: phosphorpulse config invalid — {e}"),
    }
}
pub fn render_subagent(raw: Value) {
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
        let name = identity.as_deref().or(detail);
        let mut parts = Vec::new();
        for id in &ids {
            let rendered = match *id {
                "name" => name.map(|x| (x.to_owned(), "sub.name", true)),
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
                _ => None,
            };
            if let Some((text, key, bold)) = rendered {
                let s = Segment {
                    text,
                    fg: Some(key),
                    bold,
                };
                parts.push(wrap(id, s, None, &cfg, &t, d));
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

fn num_value(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        _ => None,
    }
}
fn model_short(model: &str) -> String {
    let s = model.strip_prefix("claude-").unwrap_or(model);
    s.split('-')
        .map(|part| {
            if part.chars().all(|c| c.is_ascii_digit()) {
                part.to_owned()
            } else {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    let diff = crate::clock::now_ms() - start;
    if diff < 0 {
        return None;
    }
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
