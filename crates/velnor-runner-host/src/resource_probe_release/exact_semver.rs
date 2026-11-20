// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}
