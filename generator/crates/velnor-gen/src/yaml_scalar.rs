// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn closes_line(rest: &str) -> bool {
    let rest = rest.trim_start();
    rest.is_empty() || (rest.starts_with('#') && rest.len() != rest.trim_start_matches('#').len())
}

fn single_quoted(inner: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = inner.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch != '\'' {
            out.push(ch);
        } else if chars.peek().is_some_and(|(_, next)| *next == '\'') {
            out.push('\'');
            chars.next();
        } else {
            return closes_line(&inner[index + 1..]).then_some(out);
        }
    }
    None
}

fn escaped(chars: &mut std::str::Chars<'_>) -> Option<char> {
    let hex = |chars: &mut std::str::Chars<'_>, count: usize| {
        let digits: String = chars.take(count).collect();
        (digits.len() == count)
            .then(|| {
                u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
            })
            .flatten()
    };
    Some(match chars.next()? {
        '0' => '\0',
        'a' => '\u{7}',
        'b' => '\u{8}',
        't' | '\t' => '\t',
        'n' => '\n',
        'v' => '\u{b}',
        'f' => '\u{c}',
        'r' => '\r',
        'e' => '\u{1b}',
        ' ' => ' ',
        '"' => '"',
        '/' => '/',
        '\\' => '\\',
        'N' => '\u{85}',
        '_' => '\u{a0}',
        'L' => '\u{2028}',
        'P' => '\u{2029}',
        'x' => hex(chars, 2)?,
        'u' => hex(chars, 4)?,
        'U' => hex(chars, 8)?,
        _ => return None,
    })
}

fn double_quoted(inner: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return closes_line(chars.as_str()).then_some(out),
            '\\' => out.push(escaped(&mut chars)?),
            other => out.push(other),
        }
    }
    None
}

pub(crate) fn scalar(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'') {
        return single_quoted(inner);
    }
    if let Some(inner) = value.strip_prefix('"') {
        return double_quoted(inner);
    }
    let plain = value.split(" #").next().unwrap_or(value);
    Some(plain.trim().to_owned())
}
