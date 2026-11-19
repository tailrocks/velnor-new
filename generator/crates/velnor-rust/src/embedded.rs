// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CompileInputs, RepoPath, RepoView, RustRoot};

use crate::error::DetectError;
use crate::reads::{INPUTS_LOCK, parse_inputs_lock};

const MACROS: [&str; 3] = ["include_str", "include_bytes", "include"];

/// Files a package embeds with `include_str!`, `include_bytes!` or `include!`.
/// A path that cannot be resolved to a repository file is recorded under the
/// whole repository for that package, so any change selects it.
#[must_use]
pub(crate) fn embedded_inputs(view: &RepoView, roots: &[RustRoot]) -> CompileInputs {
    let mut inputs = CompileInputs::new();
    let sources: Vec<(&RepoPath, &[u8])> = view
        .tracked_paths
        .iter()
        .filter(|path| is_rust_source(path))
        .filter_map(|path| Some((path, view.files.get(path)?.as_slice())))
        .collect();
    for member in roots.iter().flat_map(|root| &root.members) {
        for (file, bytes) in &sources {
            if !under(member.directory.as_ref(), file) {
                continue;
            }
            for read in reads(&String::from_utf8_lossy(bytes)) {
                let key = read.and_then(|literal| resolve(file, &literal));
                inputs.entry(key).or_default().insert(member.name.clone());
            }
        }
    }
    inputs
}

fn under(directory: Option<&RepoPath>, file: &RepoPath) -> bool {
    directory.is_none_or(|directory| {
        file.as_str()
            .strip_prefix(directory.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn resolve(file: &RepoPath, literal: &str) -> Option<RepoPath> {
    if literal.starts_with('/') || literal.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = file.as_str().split('/').collect();
    parts.pop();
    for part in literal.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    RepoPath::new(parts.join("/")).ok()
}

/// `Some(literal)` for a plain string literal argument, `None` for anything else.
fn reads(text: &str) -> Vec<Option<String>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for name in MACROS {
        for (at, _) in text.match_indices(name) {
            let before = text[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || !seen.insert(at) {
                continue;
            }
            let mut i = at + name.len();
            let skip = |mut i: usize| {
                while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                    i += 1;
                }
                i
            };
            i = skip(i);
            if bytes.get(i) != Some(&b'!') {
                continue;
            }
            i = skip(i + 1);
            if !matches!(bytes.get(i), Some(b'(' | b'[' | b'{')) {
                continue;
            }
            i = skip(i + 1);
            found.push(literal(&text[i..]));
        }
    }
    found
}

fn literal(rest: &str) -> Option<String> {
    let body = rest.strip_prefix('"')?;
    let end = body.find('"')?;
    let value = &body[..end];
    (!value.contains('\\')).then(|| value.to_owned())
}

pub(crate) fn is_rust_source(path: &RepoPath) -> bool {
    std::path::Path::new(path.as_str())
        .extension()
        .is_some_and(|extension| extension == "rs")
}

pub(crate) fn locked_and_embedded(
    view: &RepoView,
    roots: &[RustRoot],
) -> Result<CompileInputs, DetectError> {
    let mut inputs = view.files.get(&INPUTS_LOCK).map_or_else(
        || Ok(CompileInputs::new()),
        |bytes| parse_inputs_lock(bytes),
    )?;
    for (file, packages) in embedded_inputs(view, roots) {
        inputs.entry(file).or_default().extend(packages);
    }
    Ok(inputs)
}

/// A Rust source file with no `include` macro cannot embed anything.
#[must_use]
pub fn is_plain_source(path: &RepoPath, bytes: &[u8]) -> bool {
    is_rust_source(path) && !bytes.windows(7).any(|window| window == b"include")
}
