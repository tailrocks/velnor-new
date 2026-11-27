// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn is_space(character: char) -> bool {
    character == '\u{feff}' || (character.is_whitespace() && character != '\u{85}')
}

pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub(super) fn trim_start(text: &str) -> &str {
    text.trim_start_matches(is_space)
}
