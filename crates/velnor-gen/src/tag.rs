// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub fn check_tag(version: &str, tag: &str) -> Result<String, String> {
    let legacy = format!("rust-ci-v{version}");
    let current = format!("tools-v{version}");
    if tag == legacy || tag == current {
        Ok(format!("tag {tag} names version {version}"))
    } else {
        Err(format!(
            "tag {tag} must be {legacy} or {current}, because this program is version {version}"
        ))
    }
}
