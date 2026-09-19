use phosphorpulse::usage::{FetchOutcome, UsageCache, UsageLimit, parse_limits, schedule};

/// REQ-02 / REQ-03 / S-01: parse scoped limits and schedule every fetch outcome.
#[test]
fn test_s01_parse_and_schedule() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/usage-api-2026-09-18.json")).unwrap();
    let fable_65 = UsageLimit {
        display_name: "Fable".to_string(),
        percent: 65.0,
        resets_at: Some(1_790_226_000_426),
        is_active: true,
    };
    let v = vec![fable_65.clone()];

    assert_eq!(parse_limits(&fixture), Some(v.clone()));

    let mut invalid_percent = fixture.clone();
    invalid_percent["limits"][2]["percent"] = serde_json::json!(150);
    assert_eq!(parse_limits(&invalid_percent), Some(vec![]));

    let mut empty_name = fixture.clone();
    empty_name["limits"][2]["scope"]["model"]["display_name"] = serde_json::json!("");
    assert_eq!(parse_limits(&empty_name), Some(vec![]));

    let mut short_name = fixture.clone();
    short_name["limits"][2]["scope"]["model"]["display_name"] = serde_json::json!("ab");
    assert_eq!(
        parse_limits(&short_name),
        Some(vec![UsageLimit {
            display_name: "ab".to_string(),
            ..fable_65.clone()
        }])
    );

    let mut long_name = fixture.clone();
    long_name["limits"][2]["scope"]["model"]["display_name"] = serde_json::json!("x".repeat(30));
    assert_eq!(
        parse_limits(&long_name),
        Some(vec![UsageLimit {
            display_name: "x".repeat(24),
            ..fable_65.clone()
        }])
    );

    assert_eq!(parse_limits(&serde_json::json!({ "limits": 5 })), None);
    assert_eq!(parse_limits(&serde_json::json!([])), None);
    assert_eq!(
        parse_limits(&serde_json::json!({ "limits": [] })),
        Some(vec![])
    );

    let now = 1_789_700_000_000_i64;
    let old = UsageCache {
        fetched_at: Some(now - 600_000),
        next_fetch_at: now - 300_000,
        limits: vec![UsageLimit {
            display_name: "Fable".to_string(),
            percent: 60.0,
            resets_at: Some(now + 1_000_000),
            is_active: true,
        }],
    };

    assert_eq!(
        schedule(FetchOutcome::Success(v.clone()), Some(&old), now, 300),
        UsageCache {
            fetched_at: Some(now),
            next_fetch_at: now + 300_000,
            limits: v.clone(),
        }
    );
    assert_eq!(
        schedule(FetchOutcome::Success(v.clone()), None, now, 120).next_fetch_at,
        now + 120_000
    );
    assert_eq!(
        schedule(FetchOutcome::RateLimited(Some(120)), Some(&old), now, 300,),
        UsageCache {
            fetched_at: old.fetched_at,
            next_fetch_at: now + 120_000,
            limits: old.limits.clone(),
        }
    );
    assert_eq!(
        schedule(FetchOutcome::RateLimited(Some(0)), Some(&old), now, 300,),
        UsageCache {
            fetched_at: old.fetched_at,
            next_fetch_at: now + 1_000,
            limits: old.limits.clone(),
        }
    );
    assert_eq!(
        schedule(
            FetchOutcome::RateLimited(Some(99_999)),
            Some(&old),
            now,
            300,
        ),
        UsageCache {
            fetched_at: old.fetched_at,
            next_fetch_at: now + 3_600_000,
            limits: old.limits.clone(),
        }
    );
    assert_eq!(
        schedule(FetchOutcome::RateLimited(None), Some(&old), now, 300),
        UsageCache {
            fetched_at: old.fetched_at,
            next_fetch_at: now + 300_000,
            limits: old.limits.clone(),
        }
    );
    assert_eq!(
        schedule(FetchOutcome::AuthFailed, Some(&old), now, 300),
        UsageCache {
            fetched_at: old.fetched_at,
            next_fetch_at: now + 300_000,
            limits: old.limits.clone(),
        }
    );
    assert_eq!(
        schedule(FetchOutcome::OtherFailure, None, now, 300),
        UsageCache {
            fetched_at: None,
            next_fetch_at: now + 30_000,
            limits: vec![],
        }
    );
    assert_eq!(
        schedule(FetchOutcome::AuthFailed, Some(&old), now, 60).next_fetch_at,
        now + 300_000
    );
}
