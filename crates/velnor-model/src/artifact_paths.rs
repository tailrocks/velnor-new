// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const SECRET_WORDS: [&str; 6] = ["key", "secret", "token", "password", "passwd", "credential"];
const SECRET_SUFFIXES: [&str; 8] = [
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".jks",
    ".keystore",
    ".gpg",
    ".asc",
];
const SECRET_PREFIXES: [&str; 5] = [".env", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];
const SECRET_NAMES: [&str; 8] = [
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
];
const PLURALS: [&str; 5] = ["keys", "secrets", "tokens", "passwords", "credentials"];
const REPOSITORY_METADATA: &str = ".git";

#[must_use]
pub fn secret_like(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| lower.contains(word))
        || SECRET_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
        || SECRET_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        || SECRET_NAMES.contains(&lower.as_str())
}

#[must_use]
pub fn secret_path(segments: &[&str]) -> bool {
    segments
        .iter()
        .any(|segment| segment.eq_ignore_ascii_case(REPOSITORY_METADATA) || secret_like(segment))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Exact,
    Folded,
}

pub const FILESYSTEM_CASE: Case = if cfg!(target_os = "linux") {
    Case::Exact
} else {
    Case::Folded
};

#[must_use]
pub fn segment_matches(pattern: &str, text: &str, case: Case) -> bool {
    let (pattern, text) = match case {
        Case::Exact => (pattern.to_owned(), text.to_owned()),
        Case::Folded => (pattern.to_ascii_lowercase(), text.to_ascii_lowercase()),
    };
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else {
        return rest.is_empty();
    };
    for part in parts {
        let Some(at) = rest.find(part) else {
            return false;
        };
        rest = &rest[at + part.len()..];
    }
    rest.ends_with(last)
}

#[must_use]
pub fn path_matches(pattern: &[&str], path: &[&str], case: Case) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", tail)) => (0..=path.len()).any(|skip| path_matches(tail, &path[skip..], case)),
        Some((head, tail)) => path.split_first().is_some_and(|(first, rest)| {
            segment_matches(head, first, case) && path_matches(tail, rest, case)
        }),
    }
}

#[must_use]
pub fn selects(pattern: &[&str], path: &[&str], case: Case) -> bool {
    (1..=path.len()).any(|length| path_matches(pattern, &path[..length], case))
}

fn refused_literals() -> impl Iterator<Item = String> {
    let suffixes = SECRET_SUFFIXES.iter().map(|suffix| format!("x{suffix}"));
    let prefixes = SECRET_PREFIXES
        .iter()
        .flat_map(|prefix| [(*prefix).to_owned(), format!("{prefix}.local")]);
    SECRET_WORDS
        .iter()
        .chain(&PLURALS)
        .chain(&SECRET_NAMES)
        .map(|name| (*name).to_owned())
        .chain(suffixes)
        .chain(prefixes)
        .chain(std::iter::once(REPOSITORY_METADATA.to_owned()))
}

#[must_use]
pub fn aliased_secret(segment_pattern: &str) -> Option<String> {
    if !segment_pattern.contains('*') || segment_pattern.chars().all(|c| c == '*') {
        return None;
    }
    refused_literals().find(|literal| segment_matches(segment_pattern, literal, Case::Folded))
}
