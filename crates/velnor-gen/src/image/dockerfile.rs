// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, RepoPath, RepoView};

use crate::ImageError;

fn hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn pinned(reference: &str) -> bool {
    reference
        .split_once("@sha256:")
        .is_some_and(|(name, digest)| !name.is_empty() && !name.contains('$') && hex(digest, 64))
}

fn escape_character(text: &str) -> char {
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(directive) = trimmed.strip_prefix('#') else {
            if trimmed.is_empty() {
                continue;
            }
            break;
        };
        if let Some((key, value)) = directive.split_once('=')
            && key.trim().eq_ignore_ascii_case("escape")
            && let Some(escape) = value.trim().chars().next()
            && (escape == '`' || escape == '\\')
        {
            return escape;
        }
        if !directive.contains('=') {
            break;
        }
    }
    '\\'
}

fn heredoc_terminators(instruction: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    for word in instruction.split_whitespace() {
        let Some(rest) = word.strip_prefix("<<") else {
            continue;
        };
        if rest.starts_with('<') {
            continue;
        }
        let (rest, strip) = rest
            .strip_prefix('-')
            .map_or((rest, false), |rest| (rest, true));
        let name = rest.trim_matches(|c| c == '"' || c == '\'');
        if !name.is_empty() {
            found.push((name.to_owned(), strip));
        }
    }
    found
}

fn lines(text: &str) -> Vec<String> {
    let escape = escape_character(text);
    let mut joined = Vec::new();
    let mut current = String::new();
    let mut raw = text.lines();
    while let Some(line) = raw.next() {
        let trimmed = line.trim();
        if current.is_empty() && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(stripped) = trimmed.strip_suffix(escape) {
            current.push_str(stripped);
            current.push(' ');
            continue;
        }
        current.push_str(trimmed);
        let instruction = std::mem::take(&mut current);
        for (name, strip) in heredoc_terminators(&instruction) {
            for body in raw.by_ref() {
                let candidate = if strip {
                    body.trim_start_matches('\t')
                } else {
                    body
                };
                if candidate == name {
                    break;
                }
            }
        }
        joined.push(instruction);
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

fn is_git(source: &str) -> bool {
    let url = source.split(['#', '?']).next().unwrap_or(source);
    source.starts_with("git@")
        || source.starts_with("git://")
        || (url.starts_with("http") && url.to_ascii_lowercase().ends_with(".git"))
}

fn commit_pinned(source: &str) -> bool {
    let fragment = source
        .split_once('#')
        .map(|(_, rest)| rest.split(':').next().unwrap_or(rest));
    let query = source
        .split_once("?ref=")
        .map(|(_, rest)| rest.split(['&', '#']).next().unwrap_or(rest));
    fragment.is_some_and(|text| hex(text, 40)) || query.is_some_and(|text| hex(text, 40))
}

fn unchecked_remote(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("ADD") {
        return None;
    }
    let rest: Vec<&str> = words.collect();
    let checksummed = rest.iter().any(|word| {
        word.strip_prefix("--checksum=sha256:")
            .is_some_and(|digest| hex(digest, 64))
    });
    let operands: Vec<&&str> = rest.iter().filter(|word| !word.starts_with("--")).collect();
    let sources = operands
        .split_last()
        .map_or(&[][..], |(_, sources)| sources);
    sources
        .iter()
        .find(|source| {
            if is_git(source) {
                !commit_pinned(source)
            } else {
                (source.starts_with("http://") || source.starts_with("https://")) && !checksummed
            }
        })
        .map(|source| (**source).to_owned())
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
    let refuse = |line: &str, reference: String| ImageError::MutableBase {
        image: image.name.clone(),
        path: image.dockerfile.clone(),
        line: line.to_owned(),
        reference,
    };
    let mut stages: Vec<String> = Vec::new();
    for line in lines(text) {
        if let Some(reference) = unchecked_remote(&line) {
            return Err(refuse(&line, reference));
        }
        for reference in sources(&line) {
            let known = reference.eq_ignore_ascii_case("scratch")
                || reference.bytes().all(|byte| byte.is_ascii_digit())
                || stages.contains(&reference.to_ascii_lowercase())
                || pinned(&reference);
            if !known {
                return Err(refuse(&line, reference));
            }
        }
        stages.extend(stage_name(&line));
    }
    Ok(())
}
