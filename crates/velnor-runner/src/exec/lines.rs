// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .as_bytes()
            .iter()
            .position(|&byte| byte == b'\r' || byte == b'\n')
            .unwrap_or(rest.len());
        let (line, tail) = rest.split_at(end);
        let separator = if tail.starts_with("\r\n") {
            2
        } else {
            usize::from(!tail.is_empty())
        };
        rest = &tail[separator..];
        Some(line)
    })
}
