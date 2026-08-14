use phosphorpulse::render::themes;
#[test]
fn test_builtin_themes_load() { for name in themes::names(){let theme=themes::builtin(name);assert_eq!(theme.rows.len(),2);assert_eq!(theme.rows[0].segments[4],"flex");assert!(theme.palette.contains_key("model"));} }
