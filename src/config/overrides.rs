use std::collections::BTreeSet;

use serde_json::Value;

use crate::jsx::number::to_number;

pub fn apply(config: &mut Value) -> BTreeSet<String> {
    let mut applied = BTreeSet::new();
    apply_node(config, &mut Vec::new(), &mut applied);
    applied
}

fn apply_node(node: &mut Value, path: &mut Vec<String>, applied: &mut BTreeSet<String>) {
    match node {
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                path.push(index.to_string());
                apply_node(value, path, applied);
                path.pop();
            }
        }
        Value::Object(values) => {
            for (key, value) in values.iter_mut() {
                path.push(key.clone());
                if value.is_array() || value.is_object() {
                    apply_node(value, path, applied);
                } else {
                    let name = env_name(path);
                    if let Ok(raw) = std::env::var(&name) {
                        *value = coerce_like(value, &raw);
                        applied.insert(path.join("."));
                    }
                }
                path.pop();
            }
        }
        _ => {}
    }
}

fn env_name(path: &[String]) -> String {
    format!(
        "PPULSE_{}",
        path.iter()
            .map(|part| screaming_snake(part))
            .collect::<Vec<_>>()
            .join("_")
    )
}

fn screaming_snake(segment: &str) -> String {
    let mut output = String::new();
    let mut previous_lower_or_digit = false;
    for character in segment.chars() {
        if character.is_ascii_uppercase() && previous_lower_or_digit {
            output.push('_');
        }
        previous_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
        output.push(character.to_ascii_uppercase());
    }
    output
}

fn coerce_like(sample: &Value, raw: &str) -> Value {
    match sample {
        Value::Bool(_) => Value::Bool(raw == "true"),
        Value::Number(_) => {
            let number = to_number(raw);
            serde_json::Number::from_f64(number)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(raw.into()))
        }
        _ => Value::String(raw.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{env_name, screaming_snake};

    #[test]
    fn derives_ppulse_names() {
        assert_eq!(screaming_snake("barWidth"), "BAR_WIDTH");
        assert_eq!(
            env_name(&["rows".into(), "0".into(), "layout".into()]),
            "PPULSE_ROWS_0_LAYOUT"
        );
    }
}
