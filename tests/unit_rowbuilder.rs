use phosphorpulse::{jsx::width::display_width, render::row_builder::{build_row, FLEX}};
#[test]
fn test_flex_layout_smoke() { let line=build_row(&["left".into(),FLEX.into(),"right".into()],"auto",20,""); assert_eq!(display_width(&line),20); assert!(line.contains("left") && line.ends_with(" right ")); }
