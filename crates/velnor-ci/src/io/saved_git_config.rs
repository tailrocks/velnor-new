// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const ALLOWED: &[(&str, &[&str])] = &[
    (
        "core",
        &[
            "repositoryformatversion",
            "filemode",
            "bare",
            "logallrefupdates",
            "ignorecase",
            "precomposeunicode",
            "symlinks",
        ],
    ),
    ("extensions", &["objectformat"]),
    ("remote", &["url", "fetch", "tagopt"]),
];
const REMOTE_SCHEMES: &[&str] = &["https://", "http://", "ssh://"];

pub(super) fn is_plain(text: &str) -> bool {
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.contains(['#', ';', '\\', '\r', '\0']) {
            return false;
        }
        let entry = if line.starts_with('[') {
            let Some((name, rest)) = header(line) else {
                return false;
            };
            section = name;
            rest
        } else {
            line
        };
        if !entry.is_empty() && !plain_entry(&section, entry) {
            return false;
        }
    }
    true
}

fn header(line: &str) -> Option<(String, &str)> {
    let close = line.find(']')?;
    let inside = &line[1..close];
    let name_end = inside
        .find(|each: char| !(each.is_ascii_alphanumeric() || each == '-'))
        .unwrap_or(inside.len());
    let (name, tail) = inside.split_at(name_end);
    let tail = tail.trim_start();
    let named = tail.is_empty()
        || tail.starts_with('.')
        || (tail.len() >= 2 && tail.starts_with('"') && tail.ends_with('"'));
    (!name.is_empty() && named && !tail.contains('\\'))
        .then(|| (name.to_ascii_lowercase(), line[close + 1..].trim()))
}

fn plain_entry(section: &str, entry: &str) -> bool {
    if entry.contains('"') {
        return false;
    }
    let (key, value) = match entry.split_once('=') {
        Some((key, value)) => (key.trim(), Some(value.trim())),
        None => (entry.trim(), None),
    };
    let key = key.to_ascii_lowercase();
    let Some((_, keys)) = ALLOWED.iter().find(|(name, _)| *name == section) else {
        return false;
    };
    keys.contains(&key.as_str()) && (key != "url" || value.is_some_and(plain_remote))
}

fn plain_remote(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    REMOTE_SCHEMES.iter().any(|scheme| {
        url.strip_prefix(scheme).is_some_and(|rest| {
            let authority = rest.split('/').next().unwrap_or("");
            !authority.is_empty() && (*scheme == "ssh://" || !authority.contains('@'))
        })
    })
}
