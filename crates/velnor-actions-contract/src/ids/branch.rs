// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub fn is_valid_branch_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "HEAD"
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with(['.', '-'])
                && !part.ends_with('.')
                && part
                    .rsplit_once('.')
                    .is_none_or(|(_, suffix)| suffix != "lock")
        })
}
