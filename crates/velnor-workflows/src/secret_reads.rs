// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expression::{is_secret, scan};

#[must_use]
pub fn reads_secret_in_text(text: &str) -> bool {
    segments(text).any(reads_secret)
}

#[must_use]
pub fn reads_secret_in_condition(text: &str) -> bool {
    if text.contains("${{") {
        reads_secret_in_text(text)
    } else {
        reads_secret(text)
    }
}

fn reads_secret(expression: &str) -> bool {
    scan(expression)
        .0
        .iter()
        .any(|reference| is_secret(reference))
}

fn segments(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let start = rest.find("${{")? + 3;
        let body = &rest[start..];
        let end = closing(body).unwrap_or(body.len());
        rest = body.get(end + 2..).unwrap_or("");
        Some(&body[..end])
    })
}

fn closing(body: &str) -> Option<usize> {
    let mut quoted = false;
    let mut chars = body.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '\'' => quoted = !quoted,
            '}' if !quoted && chars.peek().is_some_and(|(_, next)| *next == '}') => {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}
