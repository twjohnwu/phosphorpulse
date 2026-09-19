//! RED coverage for the usage refresh configuration knob.

use phosphorpulse::{
    config::model::Config,
    tui::{draft_ops, i18n},
    usage,
};
use serde_json::{Value, json};
use std::{fs, time::SystemTime};

/// REQ-07 / S-09: usage.refreshSec validates, adjusts, translates, and falls back.
#[test]
fn test_s09_refresh_sec_knob() {
    let with_refresh_sec = |value: Value| {
        let mut config = Config::defaults();
        config
            .0
            .insert("usage".into(), json!({"refreshSec": value}));
        config
    };

    let error = with_refresh_sec(json!(59))
        .validate_renderable()
        .expect_err("59 must be rejected");
    assert!(error.contains("/usage/refreshSec: must be >= 60, got 59"));

    let error = with_refresh_sec(json!(601))
        .validate_renderable()
        .expect_err("601 must be rejected");
    assert!(error.contains("must be <= 600"));
    assert!(
        with_refresh_sec(json!("300"))
            .validate_renderable()
            .is_err()
    );
    assert!(with_refresh_sec(json!(60)).validate_renderable().is_ok());
    assert!(with_refresh_sec(json!(600)).validate_renderable().is_ok());
    assert!(Config::defaults().validate_renderable().is_ok());

    let defaults = Config::defaults();
    assert_eq!(
        draft_ops::adjust_usage_refresh_sec(&defaults, 1).0["usage"]["refreshSec"],
        json!(360)
    );
    assert_eq!(
        draft_ops::adjust_usage_refresh_sec(&with_refresh_sec(json!(600)), 1).0["usage"]["refreshSec"],
        json!(600)
    );
    assert_eq!(
        draft_ops::adjust_usage_refresh_sec(&with_refresh_sec(json!(60)), -1).0["usage"]["refreshSec"],
        json!(60)
    );
    assert_eq!(
        draft_ops::adjust_usage_refresh_sec(&with_refresh_sec(json!(300)), -1).0["usage"]["refreshSec"],
        json!(240)
    );

    assert_eq!(
        draft_ops::adjust_pomodoro_work_min(&defaults, 1).0["pomodoro"]["workMin"],
        json!(30)
    );
    assert_eq!(
        draft_ops::adjust_pomodoro_refresh_sec(&defaults, 1).0["pomodoro"]["refreshSec"],
        json!(2)
    );

    let usage_label = i18n::all_keys()
        .iter()
        .find(|key| i18n::key_id(key) == "colors.usageRefreshSec");
    assert!(usage_label.is_some(), "missing colors.usageRefreshSec");
    let usage_label = usage_label.unwrap();
    assert!(!i18n::t(i18n::Lang::En, usage_label, &[]).is_empty());
    assert!(!i18n::t(i18n::Lang::ZhTw, usage_label, &[]).is_empty());
    assert!(i18n::t(i18n::Lang::En, usage_label, &[("value", "300")]).contains("300"));

    let usage_hint = i18n::all_keys()
        .iter()
        .find(|key| i18n::key_id(key) == "hints.usageRefreshSec");
    assert!(usage_hint.is_some(), "missing hints.usageRefreshSec");
    let usage_hint = usage_hint.unwrap();
    assert!(!i18n::t(i18n::Lang::En, usage_hint, &[]).is_empty());
    assert!(!i18n::t(i18n::Lang::ZhTw, usage_hint, &[]).is_empty());

    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("phosphorpulse-s09-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("settings.json"), "{not json").unwrap();
    assert_eq!(usage::read_refresh_sec(&dir), 300);
    fs::remove_dir_all(&dir).unwrap();
}
