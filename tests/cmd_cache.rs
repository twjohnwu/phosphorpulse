use phosphorpulse::cmd::{
    CommandCache, Freshness, RunOutcome, cache_key, display_text, freshness, schedule,
};

/// REQ-03 / S-02
#[test]
fn test_s02_key_schedule_display() {
    let now = 1_789_700_000_000_i64;

    let key = cache_key("k8s", Some("/a/b"));
    assert!(key.starts_with("k8s-"));
    let hex = &key["k8s-".len()..];
    assert_eq!(hex.len(), 16);
    assert!(hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_ne!(key, cache_key("k8s", Some("/a/c")));
    assert_eq!(key, cache_key("k8s", Some("/a/b")));
    assert_eq!(cache_key("k8s", None), cache_key("k8s", Some("")));

    assert_eq!(
        schedule(RunOutcome::Success("hi".into()), None, now, 5, "c"),
        CommandCache {
            fetched_at: Some(now),
            next_fetch_at: now + 5_000,
            command: "c".into(),
            output: Some("hi".into()),
        }
    );
    assert_eq!(
        schedule(RunOutcome::Success("hi".into()), None, now, 1, "c"),
        CommandCache {
            fetched_at: Some(now),
            next_fetch_at: now + 1_000,
            command: "c".into(),
            output: Some("hi".into()),
        }
    );
    let old = CommandCache {
        fetched_at: Some(now - 9_000),
        next_fetch_at: now - 1,
        command: "c".into(),
        output: Some("old".into()),
    };
    assert_eq!(
        schedule(RunOutcome::Failure, Some(&old), now, 5, "c"),
        CommandCache {
            fetched_at: Some(now - 9_000),
            next_fetch_at: now + 30_000,
            command: "c".into(),
            output: Some("old".into()),
        }
    );
    let mismatched_old = CommandCache {
        fetched_at: Some(now - 9_000),
        next_fetch_at: now - 1,
        command: "other".into(),
        output: Some("old".into()),
    };
    assert_eq!(
        schedule(
            RunOutcome::Failure,
            Some(&mismatched_old),
            now,
            5,
            "c",
        ),
        CommandCache {
            fetched_at: None,
            next_fetch_at: now + 30_000,
            command: "c".into(),
            output: None,
        }
    );
    assert_eq!(
        schedule(RunOutcome::Failure, None, now, 5, "c"),
        CommandCache {
            fetched_at: None,
            next_fetch_at: now + 30_000,
            command: "c".into(),
            output: None,
        }
    );

    assert_eq!(
        display_text("\x1b[32mgreen\x1b[0m text", false, 24),
        "green text"
    );
    assert_eq!(
        display_text("\x1b[32mgreen\x1b[0m", true, 24),
        "\x1b[32mgreen\x1b[0m"
    );
    assert_eq!(
        display_text(
            "\x1b]0;evil\x07\x1b[2J\x1b[?1049h\x1b(B\x1b[31mred\x1b[0m",
            true,
            24,
        ),
        "\x1b[31mred\x1b[0m"
    );
    let true_color = "\x1b[38;2;1;2;3mx\x1b[0m";
    assert_eq!(display_text(true_color, true, 24), true_color);
    assert_eq!(
        display_text("abcdefghijklmnop", false, 10),
        "abcdefghi…"
    );
    let wide = display_text("日本語テキスト表示", false, 10);
    assert!(wide.chars().count() <= 5);
    assert!(wide.ends_with('…'));
    assert_eq!(display_text("\x07\x1b[2J", false, 24), "");
    assert!(!display_text("a\tb", true, 24).contains('\t'));
    assert_eq!(display_text("  x  ", false, 24), "x");

    assert_eq!(freshness(Some(now - 9_000), now, 5), Freshness::Fresh);
    assert_eq!(freshness(Some(now - 61_000), now, 5), Freshness::Stale);
    assert_eq!(
        freshness(Some(now - 7_000_000), now, 3_600),
        Freshness::Fresh
    );
    assert_eq!(
        freshness(Some(now - 86_400_001), now, 5),
        Freshness::Expired
    );
    assert_eq!(freshness(Some(now + 1), now, 5), Freshness::Expired);
    assert_eq!(freshness(None, now, 5), Freshness::Expired);
}
