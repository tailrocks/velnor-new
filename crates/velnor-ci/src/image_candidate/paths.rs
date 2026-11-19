// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub fn normalize(name: &[u8]) -> (bool, Vec<&[u8]>) {
    let absolute = name.first() == Some(&b'/');
    let parts = name
        .split(|byte| *byte == b'/')
        .filter(|part| !part.is_empty() && *part != b".")
        .collect();
    (absolute, parts)
}

#[must_use]
pub fn joined(parts: &[&[u8]]) -> Vec<u8> {
    parts.join(&b'/')
}

#[must_use]
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
