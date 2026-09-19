enum State {
    Text,
    Escape {
        start: usize,
    },
    Csi {
        start: usize,
        sgr_params: bool,
        intermediates: bool,
    },
    Osc {
        saw_escape: bool,
    },
    Nf,
}

pub(crate) fn strip_escapes(raw: &str, keep_sgr: bool) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut state = State::Text;

    for (index, character) in raw.char_indices() {
        state = match state {
            State::Text => {
                if character == '\x1b' {
                    State::Escape { start: index }
                } else {
                    output.push(character);
                    State::Text
                }
            }
            State::Escape { start } => match character {
                '[' => State::Csi {
                    start,
                    sgr_params: true,
                    intermediates: false,
                },
                ']' => State::Osc { saw_escape: false },
                '\x20'..='\x2f' => State::Nf,
                '\x30'..='\x7e' => State::Text,
                '\x1b' => State::Escape { start: index },
                other => {
                    output.push(other);
                    State::Text
                }
            },
            State::Csi {
                start,
                mut sgr_params,
                mut intermediates,
            } => match character {
                '\x30'..='\x3f' if !intermediates => {
                    sgr_params &= character.is_ascii_digit() || character == ';';
                    State::Csi {
                        start,
                        sgr_params,
                        intermediates,
                    }
                }
                '\x20'..='\x2f' => {
                    intermediates = true;
                    State::Csi {
                        start,
                        sgr_params,
                        intermediates,
                    }
                }
                '\x40'..='\x7e' => {
                    if keep_sgr && sgr_params && !intermediates && character == 'm' {
                        output.push_str(&raw[start..index + character.len_utf8()]);
                    }
                    State::Text
                }
                '\x1b' => State::Escape { start: index },
                other => {
                    output.push(other);
                    State::Text
                }
            },
            State::Osc { saw_escape } => {
                if character == '\x07' || (saw_escape && character == '\\') {
                    State::Text
                } else {
                    State::Osc {
                        saw_escape: character == '\x1b',
                    }
                }
            }
            State::Nf => match character {
                '\x20'..='\x2f' => State::Nf,
                '\x30'..='\x7e' => State::Text,
                '\x1b' => State::Escape { start: index },
                other => {
                    output.push(other);
                    State::Text
                }
            },
        };
    }

    output
}
