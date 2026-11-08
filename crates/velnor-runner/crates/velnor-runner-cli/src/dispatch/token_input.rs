// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn normalize_token_line(input: &[u8]) -> Option<&[u8]> {
    let token = input
        .strip_suffix(b"\r\n")
        .or_else(|| input.strip_suffix(b"\n"))
        .unwrap_or(input);
    if token.is_empty() || !token.iter().all(u8::is_ascii_graphic) {
        return None;
    }
    Some(token)
}
