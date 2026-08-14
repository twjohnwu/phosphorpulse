/// ECMAScript `Number.prototype.toFixed(2)` for a binary double.
pub fn to_fixed_2(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == f64::INFINITY {
        return "Infinity".into();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    if value.abs() >= 1e21 {
        return scientific(value);
    }

    let negative = value.is_sign_negative() && value != 0.0;
    let cents = round_scaled(value.abs(), 100);
    let text = format!("{}.{:02}", cents / 100, cents % 100);
    if negative { format!("-{text}") } else { text }
}

// Rounds the exact IEEE-754 value times `scale`, with ties toward +infinity
// for its positive input, which is the ECMA toFixed selection rule.
fn round_scaled(value: f64, scale: u128) -> u128 {
    if value == 0.0 {
        return 0;
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let significand = if exponent == 0 {
        (bits & ((1u64 << 52) - 1)) as u128
    } else {
        ((bits & ((1u64 << 52) - 1)) | (1u64 << 52)) as u128
    };
    let shift = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    };
    let scaled = significand * scale;
    if shift >= 0 {
        return scaled << shift;
    }
    let divisor_shift = (-shift) as u32;
    if divisor_shift >= 128 {
        return 0;
    }
    let divisor = 1u128 << divisor_shift;
    let quotient = scaled / divisor;
    quotient + u128::from(scaled % divisor >= divisor / 2)
}

fn scientific(value: f64) -> String {
    let raw = format!("{value:.0e}");
    let (mantissa, exponent) = raw.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    format!("{mantissa}e{exponent:+}")
}

/// The String-input subset of ECMAScript ToNumber used by the renderer.
pub fn to_number(input: &str) -> f64 {
    let s = input.trim();
    match s {
        "" | "null" | "false" => 0.0,
        "true" => 1.0,
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => prefixed_integer(s).unwrap_or_else(|| s.parse::<f64>().unwrap_or(f64::NAN)),
    }
}

fn prefixed_integer(s: &str) -> Option<f64> {
    let (radix, digits) =
        if let Some(digits) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            (16, digits)
        } else if let Some(digits) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
            (2, digits)
        } else if let Some(digits) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
            (8, digits)
        } else {
            return None;
        };
    u128::from_str_radix(digits, radix)
        .ok()
        .map(|value| value as f64)
}
