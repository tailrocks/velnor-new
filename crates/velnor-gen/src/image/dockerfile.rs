// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, RepoPath, RepoView};

use crate::ImageError;

fn pinned(reference: &str) -> bool {
    reference
        .split_once("@sha256:")
        .is_some_and(|(name, digest)| {
            !name.is_empty()
                && !name.contains('$')
                && digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn lines(text: &str) -> Vec<String> {
    let mut joined = Vec::new();
    let mut current = String::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if current.is_empty() && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }
        if let Some(stripped) = trimmed.strip_suffix('\\') {
            current.push_str(stripped);
            current.push(' ');
        } else {
            current.push_str(trimmed);
            joined.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        joined.push(current);
    }
    joined
}

fn sources(line: &str) -> Vec<String> {
    let mut words = line.split_whitespace();
    let Some(instruction) = words.next() else {
        return Vec::new();
    };
    let rest: Vec<&str> = words.collect();
    match instruction.to_ascii_uppercase().as_str() {
        "FROM" => rest
            .iter()
            .find(|word| !word.starts_with("--"))
            .map(|word| (*word).to_owned())
            .into_iter()
            .collect(),
        "COPY" | "ADD" => rest
            .iter()
            .filter_map(|word| word.strip_prefix("--from="))
            .map(str::to_owned)
            .collect(),
        "RUN" => rest
            .iter()
            .filter_map(|word| word.strip_prefix("--mount="))
            .flat_map(|mount| mount.split(','))
            .filter_map(|field| field.strip_prefix("from="))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn stage_name(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let first = words.first()?;
    if !first.eq_ignore_ascii_case("FROM") {
        return None;
    }
    words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("AS"))
        .and_then(|at| words.get(at + 1))
        .map(|name| name.to_ascii_lowercase())
}

pub(super) fn check(view: &RepoView, image: &ImageConfig) -> Result<(), ImageError> {
    let Some(text) = RepoPath::new(image.dockerfile.as_str())
        .ok()
        .and_then(|path| view.files.get(&path))
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return Ok(());
    };
    let mut stages: Vec<String> = Vec::new();
    for line in lines(text) {
        for reference in sources(&line) {
            let known = reference.eq_ignore_ascii_case("scratch")
                || reference.bytes().all(|byte| byte.is_ascii_digit())
                || stages.contains(&reference.to_ascii_lowercase())
                || pinned(&reference);
            if !known {
                return Err(ImageError::MutableBase {
                    image: image.name.clone(),
                    path: image.dockerfile.clone(),
                    reference,
                });
            }
        }
        stages.extend(stage_name(&line));
    }
    Ok(())
}
