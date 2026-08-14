use crate::jsx::width::display_width;
use unicode_segmentation::UnicodeSegmentation;
pub const FLEX: &str = "\0FLEX\0";
pub fn build_row(segments: &[String], layout: &str, width: usize, sep: &str) -> String {
    let width = if width == 0 { 80 } else { width };
    let p: Vec<String> = segments
        .iter()
        .map(|s| {
            if s == FLEX {
                FLEX.into()
            } else {
                format!(" {s} ")
            }
        })
        .collect();
    if layout == "fixed" {
        return truncate_to_width(&p.join(sep), width);
    }
    let mut kept = Vec::new();
    let mut used = 0;
    for s in p {
        let extra = if kept.is_empty() {
            0
        } else {
            display_width(sep)
        };
        let sw = if s == FLEX { 0 } else { display_width(&s) };
        if used + extra + sw > width {
            break;
        }
        used += extra + sw;
        kept.push(s)
    }
    let fill = " ".repeat(width - used);
    let mut flex = false;
    kept.into_iter()
        .enumerate()
        .map(|(i, s)| {
            let pre = if i == 0 { "" } else { sep };
            if s == FLEX && !flex {
                flex = true;
                format!("{pre}{fill}")
            } else if s == FLEX {
                pre.into()
            } else {
                format!("{pre}{s}")
            }
        })
        .collect()
}

fn truncate_to_width(input: &str, width: usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut used = 0;
    let mut index = 0;
    let mut style_open = false;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"\x1b[") {
            let mut end = index + 2;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';') { end += 1; }
            if end < bytes.len() && bytes[end] == b'm' {
                let seq = &input[index..=end]; out.push_str(seq);
                style_open = seq != "\x1b[0m";
                index = end + 1; continue;
            }
        }
        let tail = &input[index..];
        let cluster = tail.graphemes(true).next().expect("nonempty grapheme");
        let w = display_width(cluster);
        if used + w > width { break; }
        out.push_str(cluster); used += w; index += cluster.len();
    }
    if style_open { out.push_str("\x1b[0m"); }
    out
}
