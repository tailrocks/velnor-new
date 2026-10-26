// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn valid_json_path(value: &str) -> bool {
    !value.is_empty()
        && std::path::Path::new(value)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

pub(super) fn valid_lychee_arg(value: &str) -> bool {
    !value.is_empty()
        && value != "--"
        && !value.contains("${{")
        && !value.contains(['$', '`'])
        && !value.chars().any(char::is_control)
        && !value.split_whitespace().any(|word| word == "mise")
}
