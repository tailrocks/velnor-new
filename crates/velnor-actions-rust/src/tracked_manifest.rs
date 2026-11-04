// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, path::Path};

#[must_use]
pub fn tracked_manifest_in_source(
    root: &std::path::Path,
    known: &BTreeSet<String>,
    path: &str,
) -> bool {
    let components = Path::new(path)
        .components()
        .map(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            std::path::Component::CurDir => Some(""),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(components) = components else {
        return false;
    };
    let normalized = components
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if !known.contains(&normalized) {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(target) = root.join(path).canonicalize() else {
        return false;
    };
    let Ok(relative) = target.strip_prefix(root) else {
        return false;
    };
    relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .is_some_and(|parts| known.contains(&parts.join("/")))
        && target.is_file()
}
