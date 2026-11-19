// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn validate_statements(text: &str) -> Result<(), &'static str> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = bytes[cursor..]
        .windows(2)
        .position(|marker| marker == b"{{" || marker == b"{%" || marker == b"{#")
    {
        let start = cursor + offset;
        let marker = bytes[start + 1];
        let close = match marker {
            b'{' => b"}}",
            b'%' => b"%}",
            _ => b"#}",
        };
        let end = tag_end(bytes, start + 2, *close, marker != b'#')?;
        if marker == b'%' {
            let statement = text[start + 2..end]
                .trim_start_matches(['-', ' ', '\t', '\r', '\n'])
                .split(|character: char| !character.is_ascii_alphabetic())
                .next()
                .unwrap_or_default();
            if !["if", "elif", "else", "endif"].contains(&statement) {
                return Err(
                    "unsupported template statement; use vars, expressions or conditional branches",
                );
            }
        }
        cursor = end + 2;
    }
    Ok(())
}

fn tag_end(
    bytes: &[u8],
    start: usize,
    close: [u8; 2],
    quoted: bool,
) -> Result<usize, &'static str> {
    let mut quote = None;
    let mut escaped = false;
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        let character = bytes[cursor];
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == b'\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if quoted && b"'\"`".contains(&character) {
            quote = Some(character);
        } else if quoted && bytes[cursor..].starts_with(b"__tera_") {
            return Err("unsupported internal template context");
        } else if bytes[cursor..].starts_with(&close) {
            return Ok(cursor);
        }
        cursor += 1;
    }
    Err("unsupported unterminated template tag")
}
