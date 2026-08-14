use std::io::{self, Read};

use serde_json::Value;

/// Reads the renderer protocol document from standard input.
///
/// Keeping this at the command boundary makes malformed protocol input fail
/// before any configuration work is attempted, matching the reference CLI.
pub fn read_stdin_json() -> Result<Value, String> {
    let mut input = Vec::new();
    io::stdin()
        .read_to_end(&mut input)
        .map_err(|error| format!("cannot read stdin: {error}"))?;
    serde_json::from_slice(&input).map_err(|error| error.to_string())
}
