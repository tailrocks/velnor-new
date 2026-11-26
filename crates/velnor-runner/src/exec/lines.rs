// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        let (line, tail) = rest.split_at(end);
        rest = tail
            .strip_prefix("\r\n")
            .or_else(|| tail.strip_prefix(['\r', '\n']))
            .unwrap_or(tail);
        Some(line)
    })
}
