use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WidthRow {
    input: String,
    width: usize,
}

#[derive(Debug, Deserialize)]
struct NumberRow {
    input: String,
    #[serde(rename = "toFixed2")]
    to_fixed_2: String,
    #[serde(rename = "toNumber")]
    to_number: String,
}

#[derive(Debug, Deserialize)]
struct PaletteRow {
    input: String,
    #[serde(rename = "color256")]
    color_256: String,
    #[serde(rename = "color16")]
    color_16: String,
}

fn fixture_number(value: &str) -> f64 {
    match value {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => value.parse().expect("numeric fixture value"),
    }
}

fn ansi_color_code(value: &str) -> u8 {
    value
        .rsplit(';')
        .next()
        .expect("ANSI fixture value")
        .parse()
        .expect("numeric ANSI color code")
}

fn assert_js_number_eq(index: usize, expected: f64, actual: f64) {
    assert!(
        (expected.is_nan() && actual.is_nan()) || expected == actual,
        "number fixture row {index}: expected {expected:?}, actual {actual:?}"
    );
}

/// REQ-04 / S-04: frozen TypeScript JS-semantics fixture parity.
#[test]
fn test_s04_width_number_palette_fixtures() {
    let widths: Vec<WidthRow> = serde_json::from_str(include_str!("fixtures/width-table.json"))
        .expect("valid width fixture table");
    for (index, row) in widths.iter().enumerate() {
        let actual = phosphorpulse::jsx::width::display_width(&row.input);
        assert_eq!(
            actual, row.width,
            "width fixture row {index}: expected {:?}, actual {actual:?}", row.width
        );
    }

    let numbers: Vec<NumberRow> = serde_json::from_str(include_str!("fixtures/number-table.json"))
        .expect("valid number fixture table");
    for (index, row) in numbers.iter().enumerate() {
        let expected_number = fixture_number(&row.to_number);
        let actual_number = phosphorpulse::jsx::number::to_number(&row.input);
        assert_js_number_eq(index, expected_number, actual_number);

        let actual_fixed = phosphorpulse::jsx::number::to_fixed_2(actual_number);
        assert_eq!(
            actual_fixed, row.to_fixed_2,
            "toFixed2 fixture row {index}: expected {:?}, actual {actual_fixed:?}", row.to_fixed_2
        );
    }

    let palette: Vec<PaletteRow> = serde_json::from_str(include_str!("fixtures/palette-table.json"))
        .expect("valid palette fixture table");
    for (index, row) in palette.iter().enumerate() {
        let expected = (ansi_color_code(&row.color_256), ansi_color_code(&row.color_16));
        let actual = phosphorpulse::jsx::color::downgrade(&row.input);
        assert_eq!(
            actual, expected,
            "palette fixture row {index}: expected {expected:?}, actual {actual:?}"
        );
    }
}
