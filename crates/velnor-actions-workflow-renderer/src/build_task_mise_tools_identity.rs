// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(in super::super) fn locked_github_url(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|path| {
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains("..")
            && path.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'+')
            })
    })
}

pub(in super::super) fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_' | b'-'))
}

pub(in super::super) fn exact_rust_version(value: &str) -> bool {
    let components = value.split('.').collect::<Vec<_>>();
    if components.len() != 3 {
        return false;
    }
    let (Ok(major), Ok(minor), Ok(_patch)) = (
        components[0].parse::<u32>(),
        components[1].parse::<u32>(),
        components[2].parse::<u32>(),
    ) else {
        return false;
    };
    major == 1 && minor >= 98
}

pub(in super::super) fn safe_backend(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'@')
        })
}

pub(in super::super) fn safe_option_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'-'))
}

pub(in super::super) fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'@')
        })
}
