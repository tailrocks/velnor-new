// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn is_valid(name: &str) -> bool {
    !name.is_empty()
        && name != "@"
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("@{")
        && !name.chars().any(forbidden)
        && name.split('/').all(valid_component)
}

fn forbidden(ch: char) -> bool {
    ch.is_ascii_control() || matches!(ch, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
}

fn valid_component(component: &str) -> bool {
    !component.is_empty()
        && !component.starts_with('.')
        && !component.as_bytes().ends_with(b".lock")
}
