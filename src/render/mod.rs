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
        .and_then(|x| x.parse().ok())
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
    let tasks = raw.get("tasks").and_then(Value::as_array);
    if tasks.is_none() {
        println!("phosphorpulse subagent");
        return;
    }
    for task in tasks.into_iter().flatten().take(20) {
        let name = task
            .get("name")
            .or_else(|| task.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let model = task
            .get("model")
            .and_then(Value::as_str)
            .map(|x| format!("◆ {x}"))
            .unwrap_or_default();
        let content = format!(
            " {}{}\x1b[0m  {}{}\x1b[0m ",
            color(t.palette.get("sub.name").unwrap(), d, false),
            name,
            color(t.palette.get("sub.model").unwrap(), d, false),
            model
        );
        println!(
            "{}",
            serde_json::json!({"id":task.get("id").cloned().unwrap_or(Value::Null),"content":content})
        );
    }
}
