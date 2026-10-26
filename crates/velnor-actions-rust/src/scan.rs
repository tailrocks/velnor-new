// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

pub(crate) fn strip_comment(line: &str) -> &str {
    let mut start = 0;
    while let Some(pos) = line[start..].find('#') {
        let absolute = start + pos;
        if absolute == 0
            || line[..absolute]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            return line[..absolute].trim_end();
        }
        start = absolute + 1;
    }
    line
}

pub(crate) fn find_word(line: &str, word: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = line[start..].find(word) {
        let absolute = start + pos;
        let before_ok = line[..absolute]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_word_char(ch));
        let after_ok = line[absolute + word.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !is_word_char(ch));
        if before_ok && after_ok {
            return Some(absolute);
        }
        start = absolute + word.len();
    }
    None
}

pub(crate) fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word)
        .is_some_and(|rest| rest.chars().next().is_none_or(|ch| !is_word_char(ch)))
}

pub(crate) fn has_command(line: &str, name: &str) -> bool {
    let mut search = line;
    let mut offset = 0;
    while let Some(pos) = find_word(search, name) {
        let absolute = offset + pos;
        let after = &line[absolute + name.len()..];
        let before = &line[..absolute];
        if invocation_edges(before, after) {
            return true;
        }
        offset = absolute + name.len();
        search = &line[offset..];
    }
    false
}

fn invocation_edges(before: &str, after: &str) -> bool {
    if after.starts_with('@') {
        return false;
    }
    if after.starts_with("-action") {
        return false;
    }
    !before.ends_with('/')
}

pub(crate) fn has_adjacent(line: &str, first: &str, second: &str) -> bool {
    let mut search = line;
    while let Some(pos) = find_word(search, first) {
        let rest = search[pos + first.len()..].trim_start();
        if starts_with_word(rest, second) {
            return true;
        }
        search = &search[pos + first.len()..];
    }
    false
}

pub(crate) fn has_setting(line: &str, name: &str, value: &str) -> bool {
    let mut search = line;
    while let Some(pos) = find_word(search, name) {
        let rest = &search[pos + name.len()..];
        if let Some(equals) = rest.find('=') {
            let rhs = rest[equals + 1..].trim_start();
            if starts_with_word(rhs, value) {
                return true;
            }
        }
        search = &search[pos + name.len()..];
    }
    false
}

pub(crate) fn snippet(line: &str) -> String {
    const MAX: usize = 160;
    let trimmed = line.trim();
    if trimmed.len() <= MAX {
        trimmed.to_owned()
    } else {
        trimmed.chars().take(MAX).collect()
    }
}

pub(crate) fn line_no(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}
