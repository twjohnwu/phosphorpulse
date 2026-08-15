type Rgb = [f64; 3];

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
const STANDARD: [[u8; 3]; 8] = [
    [0, 0, 0],
    [128, 0, 0],
    [0, 128, 0],
    [128, 128, 0],
    [0, 0, 128],
    [128, 0, 128],
    [0, 128, 128],
    [192, 192, 192],
];
const BRIGHT: [[u8; 3]; 8] = [
    [128, 128, 128],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [0, 0, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

pub fn downgrade(hex: &str) -> (u8, u8) {
    let rgb = parse_hex(hex);
    (nearest_256(rgb), nearest_16(rgb))
}

/// Mirrors the frozen renderer's `parseInt(clean.slice(...), 16)` conversion.
/// Invalid or absent slices deliberately become NaN: JavaScript's subsequent
/// nearest-colour comparisons leave their first candidate selected.
pub fn parse_hex(hex: &str) -> Rgb {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    [
        parse_hex_slice(hex.get(0..2)),
        parse_hex_slice(hex.get(2..4)),
        parse_hex_slice(hex.get(4..6)),
    ]
}

fn parse_hex_slice(slice: Option<&str>) -> f64 {
    let Some(slice) = slice else {
        return f64::NAN;
    };
    let slice = slice.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match slice.as_bytes().first() {
        Some(b'-') => (true, &slice[1..]),
        Some(b'+') => (false, &slice[1..]),
        _ => (false, slice),
    };
    let mut value = 0u16;
    let mut parsed = false;
    for byte in digits.bytes() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => break,
        };
        parsed = true;
        value = value * 16 + u16::from(digit);
    }
    if parsed {
        if negative {
            -(value as f64)
        } else {
            value as f64
        }
    } else {
        f64::NAN
    }
}

fn nearest_256(rgb: Rgb) -> u8 {
    let target = lab(rgb);
    let mut best = (16, f64::INFINITY);
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                let index = 16 + 36 * r + 6 * g + b;
                choose(
                    target,
                    [
                        f64::from(CUBE_LEVELS[r]),
                        f64::from(CUBE_LEVELS[g]),
                        f64::from(CUBE_LEVELS[b]),
                    ],
                    index as u8,
                    &mut best,
                );
            }
        }
    }
    for i in 0..24 {
        let v = 8 + 10 * i;
        let v = f64::from(v);
        choose(target, [v, v, v], (232 + i) as u8, &mut best);
    }
    best.0
}

fn nearest_16(rgb: Rgb) -> u8 {
    let bright = luminance(rgb) >= 0.5;
    let palette = if bright { &BRIGHT } else { &STANDARD };
    let target = lab(rgb);
    let mut best = (0, f64::INFINITY);
    for (offset, candidate) in palette.iter().enumerate() {
        choose(target, candidate.map(f64::from), offset as u8, &mut best);
    }
    (if bright { 90 } else { 30 }) + best.0
}

fn choose(target: Rgb, candidate: Rgb, index: u8, best: &mut (u8, f64)) {
    let candidate = lab(candidate);
    let distance = ((target[0] - candidate[0]).powi(2)
        + (target[1] - candidate[1]).powi(2)
        + (target[2] - candidate[2]).powi(2))
    .sqrt();
    if distance < best.1 {
        *best = (index, distance);
    }
}

fn linear(channel: f64) -> f64 {
    let c = channel / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(rgb: Rgb) -> f64 {
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn lab(rgb: Rgb) -> Rgb {
    let (r, g, b) = (linear(rgb[0]), linear(rgb[1]), linear(rgb[2]));
    let (x, y, z) = (
        r * 0.4124 + g * 0.3576 + b * 0.1805,
        r * 0.2126 + g * 0.7152 + b * 0.0722,
        r * 0.0193 + g * 0.1192 + b * 0.9505,
    );
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}
