pub mod row_builder;
pub mod themes;
use crate::{
    config,
    protocol::RenderContext,
    segments::{
        external, pomodoro,
        simple::{self, Segment, DIM},
    },
};
use serde_json::Value;
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
fn color(hex: &str, d: &str, bg: bool) -> String {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(255);
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
fn wrap(id: &str, s: Segment, cfg: &Value, t: &Theme, d: &str) -> String {
    let rowfg = cfg
        .get("segments")
        .and_then(|v| v.get(id))
        .and_then(|v| v.get("fg"))
        .and_then(Value::as_str)
        .or_else(|| s.fg)
        .unwrap_or("");
    let key = if rowfg.starts_with('#') {
        rowfg
    } else {
        t.palette.get(rowfg).map(String::as_str).unwrap_or(rowfg)
    };
    let text = s
        .text
        .replace(DIM, &color(t.palette.get("text.dim").unwrap(), d, false));
    if key.is_empty() {
        text
    } else {
        format!(
            "{}{}{}\x1b[0m",
            if s.bold { "\x1b[1m" } else { "" },
            color(key, d, false),
            text
        )
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
pub fn render_value(raw: Value, cfg: Value) -> String {
    let template = themes::builtin(
        cfg.get("activeTemplate")
            .and_then(Value::as_str)
            .unwrap_or("matrix-tron"),
    );
    let c = RenderContext::from_value(&raw, Some(&cfg));
    let config_dir = config::settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let external = external::resolve(&config_dir, c.cwd.as_deref(), true, true, true);
    let needs_pomodoro = cfg
        .get("rows")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("segments")
                    .and_then(Value::as_array)
                    .is_some_and(|segments| {
                        segments.iter().any(|id| id.as_str() == Some("pomodoro"))
                    })
            })
        });
    let pomodoro = needs_pomodoro.then(|| pomodoro::resolve(&c, &cfg, &config_dir));
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|x| x.parse::<usize>().ok().filter(|x| *x > 0).map(|x| x.saturating_sub(4).max(20)))
        .unwrap_or(80);
    let d = depth(&cfg);
    let count = cfg
        .get("rows")
        .and_then(Value::as_array)
        .map(|x| x.len())
        .unwrap_or(template.rows.len());
    (0..count)
        .map(|i| {
            let row = cfg
                .get("rows")
                .and_then(Value::as_array)
                .and_then(|x| x.get(i));
            let ids = row
                .and_then(|r| r.get("segments"))
                .and_then(Value::as_array)
                .map(|x| x.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    template
                        .rows
                        .get(i)
                        .map(|x| x.segments.iter().map(String::as_str).collect())
                        .unwrap_or_default()
                });
            let out = ids
                .into_iter()
                .filter_map(|id| {
                    if id == "flex" {
                        Some(row_builder::FLEX.into())
                    } else {
                        segment(id, &c, &external, pomodoro.as_ref())
                            .map(|s| wrap(id, s, &cfg, &template, d))
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
    let d = depth(&cfg);
    let t = themes::builtin(
        cfg.get("activeTemplate")
            .and_then(Value::as_str)
            .unwrap_or("matrix-tron"),
    );
    let c = RenderContext::from_value(&raw, Some(&cfg));
    let tasks = raw.get("tasks").and_then(Value::as_array);
    if tasks.is_none() {
        println!("phosphorpulse subagent");
        return;
    }
    let ids = cfg.get("subagent").and_then(|v| v.get("segments")).and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_else(|| t.subagent.iter().map(String::as_str).collect());
    for task in tasks.into_iter().flatten().take(20) {
        let detail = task.get("description").or_else(|| task.get("label")).and_then(Value::as_str);
        let identity = task.get("name").and_then(Value::as_str);
        let name = identity.or(detail);
        let mut parts = Vec::new();
        for id in &ids {
            let rendered = match *id {
                "name" => name.map(|x| (x.to_owned(), "sub.name", true)),
                "desc" if identity.is_some() => detail.map(|x| (x.to_owned(), "text.dim", false)),
                "model" => task.get("model").and_then(Value::as_str).map(|x| (format!("◆ {}", model_short(x)), "sub.model", true)),
                "effort" => task.get("effort").and_then(Value::as_str).map(|x| (format!("ψ {}", if x == "medium" { "med" } else { x }), "effort", false)),
                "ctx" => subagent_ctx(task, &c),
                "elapsed" => subagent_elapsed(task),
                "tokenCount" => task.get("tokenCount").and_then(num_value).map(|x| (simple::tokens(x), "text.dim", false)),
                _ => None,
            };
            if let Some((text, key, bold)) = rendered {
                let s = Segment { text, fg: Some(key), bold };
                parts.push(wrap(id, s, &cfg, &t, d));
            }
        }
        let content = parts.into_iter().map(|part| format!(" {part} ")).collect::<String>();
        let id = serde_json::to_string(task.get("id").unwrap_or(&Value::Null)).expect("serialize task id");
        let content = serde_json::to_string(&content).expect("serialize task content");
        println!("{{\"id\":{id},\"content\":{content}}}");
    }
}

fn num_value(v: &Value) -> Option<f64> { match v { Value::Number(n) => n.as_f64().filter(|x| x.is_finite()), _ => None } }
fn model_short(model: &str) -> String {
 let s = model.strip_prefix("claude-").unwrap_or(model);
 s.split('-').map(|part| if part.chars().all(|c| c.is_ascii_digit()) { part.to_owned() } else {
  let mut chars=part.chars(); chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
 }).collect::<Vec<_>>().join(" ")
}
fn subagent_ctx(task: &Value, c: &RenderContext) -> Option<(String, &'static str, bool)> {
 let tokens = task.get("tokenCount").and_then(num_value)?;
 let token_text = simple::tokens(tokens);
 if let Some(window) = task.get("contextWindowSize").and_then(num_value).filter(|x| *x > 0.) {
  let pct = ((tokens * 100. / window).floor()).min(100.);
  let fg = if pct >= c.hot_pct { "hot" } else if pct >= c.warn_pct { "warn" } else { "ok" };
  return Some((format!("⬡ {} {}% {DIM}{token_text}", gauge(pct, c.gauge_width), pct as i64), fg, false));
 }
 Some((format!("⬡ {token_text}"), "text.dim", false))
}
fn subagent_elapsed(task: &Value) -> Option<(String, &'static str, bool)> {
 let start = task.get("startTime").and_then(num_value)? as i64;
 let diff = crate::clock::now_ms() - start;
 if diff < 0 { return None; }
 let seconds = diff / 1000; let h=seconds/3600; let m=(seconds%3600)/60; let s=seconds%60;
 let text = if h > 0 { format!("⧖ {h}h{m:02}m{s:02}s") } else if m > 0 { format!("⧖ {m}m{s:02}s") } else { format!("⧖ {s}s") };
 Some((text, "sub.elapsed", false))
}
fn gauge(p:f64,w:usize)->String { let n=(p.clamp(0.,100.)*w as f64/100.).round() as usize; format!("{}{}", "▰".repeat(n), "▱".repeat(w.saturating_sub(n))) }
