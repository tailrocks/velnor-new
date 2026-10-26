// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub fn normalize(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in rel.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[must_use]
pub fn stays_inside(rel: &str) -> bool {
    !rel.starts_with('/') && normalize("", rel).is_some()
}

#[must_use]
pub fn under(dir: &str, rel: &str) -> String {
    let dir = if dir == "." { "" } else { dir };
    match (dir.is_empty(), rel.is_empty()) {
        (true, _) => rel.to_owned(),
        (false, true) => dir.to_owned(),
        (false, false) => format!("{dir}/{rel}"),
    }
}

#[must_use]
pub fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || dir == "."
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}
