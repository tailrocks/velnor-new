// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn unquoted(text: &str) -> &str {
    text.trim().trim_matches(['"', '\''])
}

fn on_value(line: &str) -> Option<&str> {
    ["on:", "\"on\":", "'on':"]
        .into_iter()
        .find_map(|prefix| line.strip_prefix(prefix))
}

fn inline_names_push(value: &str) -> bool {
    value
        .trim_matches(['[', ']', ' '])
        .split(',')
        .any(|event| unquoted(event) == "push")
}

fn block_names_push<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    let mut indent = None;
    for line in lines {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let current = line.len() - line.trim_start().len();
        if current == 0 {
            return false;
        }
        if current != *indent.get_or_insert(current) {
            continue;
        }
        let entry = line.trim().trim_start_matches('-').trim();
        if unquoted(entry.split(':').next().unwrap_or(entry)) == "push" {
            return true;
        }
    }
    false
}

pub(crate) fn runs_on_push(text: &str) -> bool {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = on_value(line) else {
            continue;
        };
        let value = rest.split(" #").next().unwrap_or(rest).trim();
        return if value.is_empty() {
            block_names_push(lines)
        } else {
            inline_names_push(value)
        };
    }
    false
}
