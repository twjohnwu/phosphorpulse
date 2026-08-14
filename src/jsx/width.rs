use unicode_segmentation::UnicodeSegmentation;

/// Matches the deliberately small East-Asian/emoji table used by the JS renderer.
/// Width is assigned to the first scalar of each grapheme, not to each scalar.
fn is_wide(cp: u32) -> bool {
    matches!(cp,
        0x1100..=0x115f | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 |
        0xf900..=0xfaff | 0xff00..=0xff60 | 0xffe0..=0xffe6 |
        0x1f300..=0x1faff | 0x20000..=0x3fffd
    )
}

/// Display-cell width compatible with `Intl.Segmenter` and the TS range table.
pub fn display_width(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut total = 0;
    let mut plain_start = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"\x1b[") {
            let mut end = index + 2;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'm' {
                total += plain_width(&input[plain_start..index]);
                index = end + 1;
                plain_start = index;
                continue;
            }
        }
        index += 1;
    }

    total + plain_width(&input[plain_start..])
}

fn plain_width(input: &str) -> usize {
    input
        .graphemes(true)
        .map(|cluster| usize::from(is_wide(cluster.chars().next().unwrap() as u32)) + 1)
        .sum()
}
