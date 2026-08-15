//! RED coverage for TUI draft editing operations.

use phosphorpulse::config::model::Config;
use serde_json::json;

#[path = "../src/tui/draft_ops.rs"]
mod draft_ops;

use draft_ops::{
    add_row, add_subagent_segment, adjust_dir_path_depth, adjust_gauge_bar_width,
    adjust_gauge_hot_pct, adjust_gauge_warn_pct, adjust_pomodoro_refresh_sec,
    adjust_pomodoro_work_min, cycle_color_depth, cycle_segment_fg, delete_row_segment,
    delete_subagent_segment, insert_row_segment, insert_subagent_segment, move_row_segment,
    move_subagent_segment, toggle_row_layout,
};

fn fixture() -> Config {
    Config(
        json!({
            "colorDepth": "auto",
            "rows": [
                {"layout": "auto", "segments": ["model", "pomodoro", "ctx"]},
                {"layout": "fixed", "segments": ["dir"]}
            ],
            "pomodoro": {"workMin": 90, "refreshSec": 60},
            "segments": {"dir": {"fg": "#00FF41", "pathDepth": 99}},
            "gauge": {"barWidth": 80, "warnPct": 100, "hotPct": 100},
            "subagent": {"segments": ["name", "model", "elapsed"]}
        })
        .as_object()
        .unwrap()
        .clone(),
    )
}

/// REQ-03 / S-16: draft-ops matches the frozen RowsSegments, ColorsTheme,
/// and SubagentLine editing semantics without terminal I/O.
#[test]
fn test_s16_draft_ops_semantics() {
    let draft = fixture();

    let added = add_row(&draft, vec!["git".into()]);
    assert_eq!(
        added.0["rows"],
        json!([
            {"layout": "auto", "segments": ["model", "pomodoro", "ctx"]},
            {"layout": "fixed", "segments": ["dir"]},
            {"layout": "auto", "segments": ["git"]}
        ])
    );
    assert_eq!(
        add_row(&added, vec!["node".into()]).0,
        added.0,
        "fourth row is rejected"
    );

    let inserted = insert_row_segment(&draft, 0, 1, "git");
    assert_eq!(
        inserted.0["rows"][0]["segments"],
        json!(["model", "git", "pomodoro", "ctx"])
    );
    let deleted = delete_row_segment(&inserted, 0, 1);
    assert_eq!(
        deleted.0["rows"][0]["segments"],
        json!(["model", "pomodoro", "ctx"])
    );
    let moved = move_row_segment(&draft, 0, 0, -1);
    assert_eq!(
        moved.0["rows"][0]["segments"],
        json!(["ctx", "pomodoro", "model"])
    );
    assert_eq!(
        toggle_row_layout(&draft, 0).0["rows"][0]["layout"],
        json!("fixed")
    );
    assert_eq!(
        insert_row_segment(&draft, 9, 0, "git").0,
        draft.0,
        "invalid row is ignored"
    );

    assert_eq!(
        adjust_pomodoro_work_min(&draft, 1).0["pomodoro"]["workMin"],
        json!(90)
    );
    assert_eq!(
        adjust_pomodoro_work_min(&draft, -99).0["pomodoro"]["workMin"],
        json!(5)
    );
    assert_eq!(
        adjust_pomodoro_refresh_sec(&draft, 1).0["pomodoro"]["refreshSec"],
        json!(60)
    );
    assert_eq!(
        adjust_pomodoro_refresh_sec(&draft, -99).0["pomodoro"]["refreshSec"],
        json!(1)
    );

    assert_eq!(
        cycle_color_depth(&draft, -1).0["colorDepth"],
        json!("truecolor")
    );
    assert_eq!(
        cycle_color_depth(&draft, 1).0["colorDepth"],
        json!("16color")
    );
    assert_eq!(
        cycle_segment_fg(&draft, "dir", -1).0["segments"]["dir"]["fg"],
        json!("#282D2A")
    );
    assert_eq!(
        cycle_segment_fg(&draft, "dir", 1).0["segments"]["dir"]["fg"],
        json!("#00CF41")
    );
    assert_eq!(
        adjust_gauge_bar_width(&draft, 1).0["gauge"]["barWidth"],
        json!(80)
    );
    assert_eq!(
        adjust_gauge_bar_width(&draft, -99).0["gauge"]["barWidth"],
        json!(1)
    );
    assert_eq!(
        adjust_gauge_warn_pct(&draft, 1).0["gauge"]["warnPct"],
        json!(100)
    );
    assert_eq!(
        adjust_gauge_hot_pct(&draft, -1).0["gauge"]["hotPct"],
        json!(100)
    );
    assert_eq!(
        adjust_dir_path_depth(&draft, 1).0["segments"]["dir"]["pathDepth"],
        json!(99)
    );
    assert_eq!(
        adjust_dir_path_depth(&draft, -99).0["segments"]["dir"]["pathDepth"],
        json!(1)
    );

    let sub_added = add_subagent_segment(&draft, "cwd");
    assert_eq!(
        sub_added.0["subagent"]["segments"],
        json!(["name", "model", "elapsed", "cwd"])
    );
    let sub_inserted = insert_subagent_segment(&draft, 1, "cwd");
    assert_eq!(
        sub_inserted.0["subagent"]["segments"],
        json!(["name", "cwd", "model", "elapsed"])
    );
    let sub_deleted = delete_subagent_segment(&sub_inserted, 1);
    assert_eq!(
        sub_deleted.0["subagent"]["segments"],
        json!(["name", "model", "elapsed"])
    );
    let sub_moved = move_subagent_segment(&draft, 0, -1);
    assert_eq!(
        sub_moved.0["subagent"]["segments"],
        json!(["elapsed", "model", "name"])
    );
    assert_eq!(
        delete_subagent_segment(&draft, 9).0,
        draft.0,
        "invalid subagent index is ignored"
    );
}
