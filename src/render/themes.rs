use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct Theme {
    pub rows: Vec<Row>,
    pub subagent: Vec<String>,
    pub palette: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub layout: String,
    pub segments: Vec<String>,
}
fn palette(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}
fn common(p: BTreeMap<String, String>) -> Theme {
    Theme {
        rows: vec![
            Row {
                layout: "auto".into(),
                segments: vec!["model", "effort", "git", "dir", "flex", "ctx"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
            Row {
                layout: "auto".into(),
                segments: vec![
                    "limit5h", "pomodoro", "node", "flex", "burn", "version", "limit7d",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ],
        subagent: vec!["name", "desc", "model", "effort", "ctx", "elapsed"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        palette: p,
    }
}
pub fn builtin(name: &str) -> Theme {
    match name {
        "solarized-dark" => common(palette(&[
            ("dir", "#268BD2"),
            ("git.ok", "#2AA198"),
            ("git.dirty", "#B58900"),
            ("model", "#93A1A1"),
            ("version", "#586E75"),
            ("ctx.ok", "#859900"),
            ("cost", "#657B83"),
            ("clock", "#CB4B16"),
            ("effort", "#6C71C4"),
            ("node", "#859900"),
            ("python", "#268BD2"),
            ("sub.name", "#859900"),
            ("sub.model", "#B58900"),
            ("sub.ctx", "#2AA198"),
            ("sub.elapsed", "#CB4B16"),
            ("text", "#839496"),
            ("text.dim", "#586E75"),
            ("ok", "#859900"),
            ("warn", "#B58900"),
            ("hot", "#DC322F"),
            ("pomodoro.work", "#859900"),
            ("pomodoro.break", "#2AA198"),
        ])),
        "solarized-light" => common(palette(&[
            ("dir", "#268BD2"),
            ("git.ok", "#2AA198"),
            ("git.dirty", "#B58900"),
            ("model", "#586E75"),
            ("version", "#93A1A1"),
            ("ctx.ok", "#859900"),
            ("cost", "#839496"),
            ("clock", "#CB4B16"),
            ("effort", "#6C71C4"),
            ("node", "#859900"),
            ("python", "#268BD2"),
            ("sub.name", "#859900"),
            ("sub.model", "#B58900"),
            ("sub.ctx", "#2AA198"),
            ("sub.elapsed", "#CB4B16"),
            ("text", "#657B83"),
            ("text.dim", "#93A1A1"),
            ("ok", "#859900"),
            ("warn", "#B58900"),
            ("hot", "#DC322F"),
            ("pomodoro.work", "#859900"),
            ("pomodoro.break", "#2AA198"),
        ])),
        _ => common(palette(&[
            ("dir", "#00CF41"),
            ("git.ok", "#00CDCD"),
            ("git.dirty", "#00FFFF"),
            ("model", "#00CF41"),
            ("version", "#969696"),
            ("ctx.ok", "#00CDCD"),
            ("gauge.ctx-well", "#121612"),
            ("gauge.5h-well", "#0E120E"),
            ("gauge.7d-well", "#0A0E0A"),
            ("cost", "#282D2A"),
            ("clock", "#003232"),
            ("effort", "#008F11"),
            ("node", "#00E5FF"),
            ("python", "#00E5FF"),
            ("sub.name", "#00FFFF"),
            ("sub.model", "#FFD700"),
            ("sub.ctx", "#00FFFF"),
            ("sub.elapsed", "#FF7F50"),
            ("text", "#00FF41"),
            ("text.dim", "#008F11"),
            ("ok", "#00FF41"),
            ("warn", "#FF7F50"),
            ("hot", "#FF3737"),
            ("pomodoro.work", "#00CF41"),
            ("pomodoro.break", "#00CDCD"),
        ])),
    }
}
pub fn names() -> [&'static str; 3] {
    ["matrix-tron", "solarized-dark", "solarized-light"]
}
