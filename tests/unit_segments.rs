use phosphorpulse::{protocol::RenderContext, segments::simple};

#[test]
fn test_simple_segments_smoke() {
    let c = RenderContext { model_display_name: Some("Fable".into()), effort_level: Some("medium".into()), cwd: Some("/tmp/demo".into()), version: Some("1.2.3".into()), context_used_percentage: Some(42.), total_cost_usd: Some(1.25), total_duration_ms: Some(60_000.), five_hour: phosphorpulse::protocol::Window { used_percentage: Some(10.), resets_at: None }, seven_day: phosphorpulse::protocol::Window { used_percentage: Some(20.), resets_at: None }, ..Default::default() };
    assert!(simple::model(&c).unwrap().text.starts_with('◆'));
    assert_eq!(simple::effort(&c).unwrap().text, "ψ med");
    assert!(simple::dir(&c).is_some() && simple::ctx(&c).is_some());
    assert!(simple::limit5h(&c).is_some() && simple::limit7d(&c).is_some());
    assert!(simple::version(&c).is_some() && simple::cost(&c).is_some() && simple::burn(&c).is_some());
}
