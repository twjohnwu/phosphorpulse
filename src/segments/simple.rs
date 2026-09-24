use crate::{
    clock,
    jsx::number::to_fixed_2,
    protocol::{RenderContext, Window},
    render::row_builder::truncate_to_width,
};
use crate::jsx::width::display_width;
pub const DIM: &str = "\0text-dim\0";
pub(crate) const MAX_STATUS_WIDTH: usize = 24;

struct StatusText {
    session_prefix: &'static str,
    fast: &'static str,
    output_style_prefix: &'static str,
    thinking: &'static str,
}

const STATUS_TEXT: [StatusText; 2] = [
    StatusText {
        session_prefix: "#",
        fast: "fast",
        output_style_prefix: "style:",
        thinking: "no-think",
    },
    StatusText {
        session_prefix: "\u{f02b} ",
        fast: "\u{f0e7} fast",
        output_style_prefix: "\u{f1fc} ",
        thinking: "\u{f05e} think",
    },
];
#[derive(Clone, Debug)]
pub struct Segment {
    pub text: String,
    pub fg: Option<&'static str>,
    pub bold: bool,
}
fn gauge(p: f64, w: usize) -> String {
    let p = p.clamp(0., 100.);
    let n = (p * w as f64 / 100.).round() as usize;
    format!("{}{}", "▰".repeat(n), "▱".repeat(w.saturating_sub(n)))
}
fn gauge_color(p: f64, c: &RenderContext, ctx: bool) -> &'static str {
    if p >= c.hot_pct {
        "hot"
    } else if p >= c.warn_pct {
        "warn"
    } else if ctx {
        "ctx.ok"
    } else {
        "ok"
    }
}
pub fn tokens(n: f64) -> String {
    let n = n.trunc() as i64;
    if n >= 1_000_000 {
        format!("{}.{}M", n / 1_000_000, (n % 1_000_000) / 100_000)
    } else if n >= 1000 {
        format!("{}.{}k", n / 1000, (n % 1000) / 100)
    } else {
        n.to_string()
    }
}
pub fn model(c: &RenderContext) -> Option<Segment> {
    c.model_display_name.as_ref().map(|v| Segment {
        text: format!("◆ {v}"),
        fg: Some("model"),
        bold: true,
    })
}
pub fn effort(c: &RenderContext) -> Option<Segment> {
    c.effort_level.as_ref().map(|v| Segment {
        text: format!("ψ {}", if v == "medium" { "med" } else { v }),
        fg: Some("effort"),
        bold: false,
    })
}
pub fn dir(c: &RenderContext) -> Option<Segment> {
    let mut p = c.cwd.clone()?;
    if let Ok(h) = std::env::var("HOME") {
        if p == h {
            p = "~".into()
        } else if p.starts_with(&(h.clone() + "/")) {
            p = format!("~{}", &p[h.len()..])
        }
    }
    let abs = p.starts_with('/');
    let a: Vec<_> = p.split('/').filter(|x| !x.is_empty()).collect();
    let k = if c.path_depth > 0 && c.path_depth < a.len() {
        &a[a.len() - c.path_depth..]
    } else {
        &a
    };
    Some(Segment {
        text: format!("{}{}", if abs { "/" } else { "" }, k.join("/")),
        fg: Some("dir"),
        bold: true,
    })
}
pub fn ctx(c: &RenderContext) -> Option<Segment> {
    let p = c.context_used_percentage?;
    let mut t = format!("⬡ {} {}%", gauge(p, c.gauge_width), p.round() as i64);
    if let (Some(a), Some(b), Some(d), Some(e)) = (
        c.total_input_tokens,
        c.total_output_tokens,
        c.cache_read_input_tokens,
        c.cache_creation_input_tokens,
    ) {
        t += &format!(
            " {DIM}↑{} ↓{} cr:{} cw:{}",
            tokens(a),
            tokens(b),
            tokens(d),
            tokens(e)
        );
    }
    Some(Segment {
        text: t,
        fg: Some(gauge_color(p, c, true)),
        bold: false,
    })
}
fn countdown(r: i64) -> String {
    let s = (r - clock::now_ms()) / 1000;
    if s <= 0 {
        return "now".into();
    }
    let d = s / 86400;
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    if d > 0 {
        format!("{d}d{h:02}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}
fn limit(label: &str, w: &Window, c: &RenderContext) -> Option<Segment> {
    w.used_percentage
        .map(|p| limit_parts(label, p, w.resets_at, c))
}
pub(crate) fn limit_parts(
    label: &str,
    percent: f64,
    resets_at: Option<i64>,
    c: &RenderContext,
) -> Segment {
    let mut t = format!(
        "{label} {} {}%",
        gauge(percent, c.gauge_width),
        percent.round() as i64
    );
    if let Some(r) = resets_at {
        t += &format!(" {DIM}↺{}", countdown(r));
    }
    Segment {
        text: t,
        fg: Some(gauge_color(percent, c, false)),
        bold: false,
    }
}

enum Freshness {
    Fresh,
    Stale,
    Expired,
}

pub(crate) fn limit_model(
    cache: Option<&crate::usage::UsageCache>,
    now: i64,
    c: &RenderContext,
) -> Segment {
    let freshness = match cache
        .and_then(|value| value.fetched_at)
        .and_then(|fetched_at| now.checked_sub(fetched_at))
    {
        Some(0..=1_800_000) => Freshness::Fresh,
        Some(1_800_001..=86_400_000) => Freshness::Stale,
        _ => Freshness::Expired,
    };
    // Prefer the limit for the model in use; otherwise fall back to the first
    // per-model weekly limit so the gauge stays visible, dimmed.
    let active = cache.and_then(|value| value.limits.iter().find(|limit| limit.is_active));
    let shown = active.or_else(|| cache.and_then(|value| value.limits.first()));

    match (freshness, shown) {
        (Freshness::Fresh, Some(limit)) if limit.is_active => {
            limit_parts(&limit.display_name, limit.percent, limit.resets_at, c)
        }
        (Freshness::Fresh | Freshness::Stale, Some(limit)) => {
            let mut segment = limit_parts(&limit.display_name, limit.percent, limit.resets_at, c);
            segment.fg = Some("text.dim");
            segment
        }
        _ => Segment {
            text: "--".into(),
            fg: Some("text.dim"),
            bold: false,
        },
    }
}
pub fn limit5h(c: &RenderContext) -> Option<Segment> {
    limit("5h", &c.five_hour, c)
}
pub fn limit7d(c: &RenderContext) -> Option<Segment> {
    limit("7d", &c.seven_day, c)
}
pub fn version(c: &RenderContext) -> Option<Segment> {
    c.version.as_ref().map(|v| Segment {
        text: format!("v{v}"),
        fg: Some("version"),
        bold: false,
    })
}
fn status_text(c: &RenderContext) -> &'static StatusText {
    &STATUS_TEXT[usize::from(c.nerd_font)]
}
fn cap_status_text(prefix: &str, value: &str) -> String {
    let full = format!("{prefix}{value}");
    if display_width(&full) <= MAX_STATUS_WIDTH {
        full
    } else {
        format!(
            "{prefix}{}…",
            truncate_to_width(
                value,
                MAX_STATUS_WIDTH - display_width(prefix) - 1,
            )
        )
    }
}
pub fn session(c: &RenderContext) -> Option<Segment> {
    c.session_name.as_ref().map(|name| Segment {
        text: cap_status_text(status_text(c).session_prefix, name),
        fg: Some("dir"),
        bold: false,
    })
}
pub fn fast_mode(c: &RenderContext) -> Option<Segment> {
    (c.fast_mode == Some(true)).then(|| Segment {
        text: status_text(c).fast.to_owned(),
        fg: Some("warn"),
        bold: false,
    })
}
pub fn output_style(c: &RenderContext) -> Option<Segment> {
    c.output_style
        .as_ref()
        .filter(|name| name.as_str() != "default")
        .map(|name| Segment {
            text: cap_status_text(status_text(c).output_style_prefix, name),
            fg: Some("version"),
            bold: false,
        })
}
pub fn thinking(c: &RenderContext) -> Option<Segment> {
    (c.thinking_enabled == Some(false)).then(|| Segment {
        text: status_text(c).thinking.to_owned(),
        fg: Some("warn"),
        bold: false,
    })
}
pub fn cost(c: &RenderContext) -> Option<Segment> {
    match c.total_cost_usd {
        Some(x) if x != 0. => Some(Segment {
            text: format!("${}", to_fixed_2(x)),
            fg: Some("cost"),
            bold: false,
        }),
        _ => None,
    }
}
pub fn burn(c: &RenderContext) -> Option<Segment> {
    let d = c.total_duration_ms?;
    if d <= 0. {
        return None;
    }
    let mut best: Option<(&str, f64)> = None;
    for (n, w) in [("5h", &c.five_hour), ("7d", &c.seven_day)] {
        if let Some(p) = w.used_percentage.filter(|p| *p > 0.) {
            let eta = (100. - p) / (p / d);
            if best.map(|x| eta < x.1).unwrap_or(true) {
                best = Some((n, eta))
            }
        }
    }
    let (n, e) = best?;
    let s = (e / 1000.).round().max(0.) as i64;
    let days = s / 86400;
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    Some(Segment {
        text: format!(
            "⚡ {n} ⇢ {}",
            if days > 0 {
                format!("{days}d{h:02}h")
            } else if h > 0 {
                format!("{h}h{m:02}m")
            } else {
                format!("{m}m")
            }
        ),
        fg: Some("burn"),
        bold: false,
    })
}
