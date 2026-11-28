// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) struct Split {
    pub primary: String,
    pub twin: String,
}

pub(super) fn split_overflow(json: &str) -> Option<Split> {
    let mut primary = String::with_capacity(json.len());
    let mut twin = String::with_capacity(json.len());
    let mut found = false;
    let mut copied = 0;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, c) in json.char_indices().chain([(json.len(), ' ')]) {
        let number_char = !in_string && matches!(c, '0'..='9' | '+' | '-' | '.' | 'e' | 'E');
        match (start, number_char) {
            (None, true) => start = Some(offset),
            (Some(from), false) => {
                start = None;
                let run = &json[from..offset];
                if overflows(run) {
                    found = true;
                    primary.push_str(&json[copied..from]);
                    twin.push_str(&json[copied..from]);
                    let sign = if run.starts_with('-') { "-" } else { "" };
                    let pad = " ".repeat(run.len() - sign.len() - 1);
                    primary.extend([sign, "1", pad.as_str()]);
                    twin.extend([sign, "2", pad.as_str()]);
                    copied = offset;
                }
            }
            _ => {}
        }
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
        }
    }
    found.then(|| {
        primary.push_str(&json[copied..]);
        twin.push_str(&json[copied..]);
        Split { primary, twin }
    })
}

fn overflows(run: &str) -> bool {
    is_json_number(run) && run.parse::<f64>().is_ok_and(f64::is_infinite)
}

fn is_json_number(text: &str) -> bool {
    let digits = |value: &str| value.bytes().take_while(u8::is_ascii_digit).count();
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let integer = digits(unsigned);
    if integer == 0 || (integer > 1 && unsigned.starts_with('0')) {
        return false;
    }
    let mut rest = &unsigned[integer..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = digits(fraction);
        if count == 0 {
            return false;
        }
        rest = &fraction[count..];
    }
    match rest.strip_prefix(['e', 'E']) {
        None => rest.is_empty(),
        Some(exponent) => {
            let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            !exponent.is_empty() && exponent.bytes().all(|byte| byte.is_ascii_digit())
        }
    }
}
