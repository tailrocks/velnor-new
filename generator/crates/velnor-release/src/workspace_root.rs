// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;

use crate::error::ReleaseError;

type Parse = fn(&BTreeMap<String, String>, &str) -> Result<Value, ReleaseError>;

fn manifest_path(directory: &str) -> String {
    if directory.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{directory}/Cargo.toml")
    }
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn segment_matches(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| segment_matches(rest, &text[skip..])),
        Some((b'?', rest)) => !text.is_empty() && segment_matches(rest, &text[1..]),
        Some((byte, rest)) => text.first() == Some(byte) && segment_matches(rest, &text[1..]),
    }
}

fn glob_matches(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| glob_matches(rest, &path[skip..])),
        Some((segment, rest)) => {
            path.first()
                .is_some_and(|part| segment_matches(segment.as_bytes(), part.as_bytes()))
                && glob_matches(rest, &path[1..])
        }
    }
}

fn listed(workspace: &Value, key: &str, relative: &str) -> Vec<bool> {
    let path: Vec<&str> = relative.split('/').collect();
    workspace
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|entry| {
            let entry = normalize(entry);
            let pattern: Vec<&str> = entry.split('/').collect();
            glob_matches(&pattern, &path)
        })
        .collect()
}

fn includes(workspace: &Value, root_directory: &str, member_directory: &str) -> bool {
    let relative = if root_directory.is_empty() {
        member_directory
    } else {
        match member_directory
            .strip_prefix(root_directory)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            Some(rest) => rest,
            None => return false,
        }
    };
    if relative.is_empty() {
        return true;
    }
    let mut prefixes = relative
        .match_indices('/')
        .map(|(index, _)| &relative[..index]);
    let excluded = listed(workspace, "exclude", relative).contains(&true)
        || prefixes.any(|prefix| listed(workspace, "exclude", prefix).contains(&true));
    !excluded && listed(workspace, "members", relative).contains(&true)
}

pub(crate) fn workspace_root(
    manifests: &BTreeMap<String, String>,
    manifest: &str,
    package: &Value,
    parse: Parse,
) -> Result<Option<String>, ReleaseError> {
    let own = manifest
        .strip_suffix("Cargo.toml")
        .map(|d| d.trim_end_matches('/'));
    let Some(own) = own else { return Ok(None) };
    if let Some(declared) = package.get("workspace").and_then(Value::as_str) {
        let joined = if own.is_empty() {
            declared.to_owned()
        } else {
            format!("{own}/{declared}")
        };
        let path = manifest_path(&normalize(&joined));
        return Ok(manifests.contains_key(&path).then_some(path));
    }
    let mut directory = Some(own);
    while let Some(current) = directory {
        let candidate = manifest_path(current);
        if manifests.contains_key(&candidate) {
            let document = parse(manifests, &candidate)?;
            if let Some(workspace) = document.get("workspace")
                && (current == own || includes(workspace, current, own))
            {
                return Ok(Some(candidate));
            }
        }
        directory = match current {
            "" => None,
            _ => Some(current.rsplit_once('/').map_or("", |(parent, _)| parent)),
        };
    }
    Ok(None)
}

pub(crate) fn unparsable_package_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some(value) = line.strip_prefix("name").map(str::trim_start)
            && let Some(value) = value.strip_prefix('=')
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}
