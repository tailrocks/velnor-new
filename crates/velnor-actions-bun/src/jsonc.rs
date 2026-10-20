//! JSON with comments and trailing commas (`bun.lock`, `tsconfig.json`).

use crate::BunError;

/// Plain JSON text equivalent to `text`: comments and trailing commas are
/// removed outside string literals; every other byte is preserved.
///
/// # Errors
///
/// Returns `jsonc_unterminated` for an unterminated string or block comment.
pub fn to_json(text: &str) -> Result<String, BunError> {
    let stripped = strip_comments(text)?;
    Ok(drop_trailing_commas(&stripped))
}

fn strip_comments(text: &str) -> Result<String, BunError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            match ch {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (ch, chars.peek().copied()) {
            ('"', _) => {
                in_string = true;
                out.push(ch);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                skip_block(&mut chars)?;
                out.push(' ');
            }
            _ => out.push(ch),
        }
    }
    if in_string {
        return Err(BunError::bare("jsonc_unterminated"));
    }
    Ok(out)
}

fn skip_block(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<(), BunError> {
    let mut previous = '\0';
    for next in chars.by_ref() {
        if previous == '*' && next == '/' {
            return Ok(());
        }
        previous = next;
    }
    Err(BunError::bare("jsonc_unterminated"))
}

fn drop_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            out.push(ch);
            if ch == '\\' {
                index += 1;
                out.extend(chars.get(index));
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
            out.push(ch);
        } else if ch == ',' && closes_next(&chars[index + 1..]) {
            // Trailing comma: omitted.
        } else {
            out.push(ch);
        }
        index += 1;
    }
    out
}

fn closes_next(rest: &[char]) -> bool {
    rest.iter()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, '}' | ']'))
}
