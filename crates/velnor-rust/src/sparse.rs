// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{CheckoutScope, PathGlob, RepoFacts, RepoPath, RustRoot, Unread};

const TOP_FILES: [&str; 12] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "mise.toml",
    "mise.lock",
    ".mise.toml",
    ".tool-versions",
    "deny.toml",
    "rustfmt.toml",
    "clippy.toml",
    ".rustfmt.toml",
];
const TOP_DIRECTORIES: [&str; 3] = [".cargo", ".config", ".velnor"];
const EVERY_MANIFEST: &str = "**/Cargo.toml";

pub(crate) fn paths(facts: &RepoFacts, roots: &[RustRoot]) -> Option<Vec<String>> {
    if facts.config.checkout == CheckoutScope::Full {
        return None;
    }
    listing(facts, roots)
}

fn listing(facts: &RepoFacts, roots: &[RustRoot]) -> Option<Vec<String>> {
    if roots.is_empty()
        || !facts.config.test_setup.is_empty()
        || facts.compile_inputs.contains_key(&None)
    {
        return None;
    }
    let mut paths: BTreeSet<String> = TOP_FILES.iter().map(|file| format!("/{file}")).collect();
    paths.extend(TOP_DIRECTORIES.iter().map(|dir| format!("/{dir}/")));
    paths.insert(EVERY_MANIFEST.to_owned());
    for root in roots {
        let directories = std::iter::once(&root.manifest)
            .chain(&root.depends_on)
            .chain(root.members.iter().map(|member| &member.manifest))
            .chain(
                root.members
                    .iter()
                    .filter_map(|member| member.directory.as_ref()),
            );
        for manifest in directories {
            paths.insert(directory(manifest)?);
        }
        paths.extend(ancestor_cargo_directories(&root.manifest));
    }
    paths.extend(unowned_directories(facts, roots)?);
    paths.extend(toolchain_directory(facts)?);
    paths.extend(
        facts
            .compile_inputs
            .keys()
            .flatten()
            .map(|path| format!("/{path}")),
    );
    paths.extend(
        facts
            .config
            .inputs
            .iter()
            .map(|rule| format!("/{}", escaped(rule.glob.as_str()))),
    );
    Some(paths.into_iter().collect())
}

fn ancestor_cargo_directories(manifest: &RepoPath) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut folder = manifest.parent().and_then(|folder| folder.parent());
    while let Some(current) = folder {
        ancestors.push(format!("/{current}/.cargo/"));
        folder = current.parent();
    }
    ancestors
}

fn unowned_directories(facts: &RepoFacts, roots: &[RustRoot]) -> Option<Vec<String>> {
    let mut pending: Vec<&RepoPath> = roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| &member.dependencies)
        .map(|dependency| &dependency.manifest)
        .collect();
    let mut seen = BTreeSet::new();
    let mut directories = Vec::new();
    while let Some(manifest) = pending.pop() {
        let Some(package) = facts.unowned.get(manifest) else {
            continue;
        };
        if !seen.insert(manifest) {
            continue;
        }
        if package
            .unread
            .as_ref()
            .is_some_and(|unread| !matches!(unread, Unread::Git(_) | Unread::Outside(_)))
        {
            return None;
        }
        directories.push(directory(manifest)?);
        pending.extend(package.dependencies.iter().map(|edge| &edge.manifest));
    }
    Some(directories)
}

fn toolchain_directory(facts: &RepoFacts) -> Option<Vec<String>> {
    let Some(path) = facts
        .toolchain
        .as_ref()
        .and_then(|toolchain| toolchain.path.as_deref())
        .filter(|path| !path.starts_with('/'))
    else {
        return Some(Vec::new());
    };
    let path = path
        .strip_prefix("./")
        .unwrap_or(path)
        .trim_end_matches('/');
    if path.split('/').any(|part| matches!(part, "" | "." | "..")) {
        return None;
    }
    Some(vec![format!("/{}/", escaped(path))])
}

fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '?' | '[' | ']' | '\\' | ' ') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

fn directory(manifest: &RepoPath) -> Option<String> {
    let text = manifest.as_str();
    match text.strip_suffix("Cargo.toml") {
        None => Some(format!("/{text}/")),
        Some("") => None,
        Some(parent) => Some(format!("/{parent}")),
    }
}

#[must_use]
pub fn bytes_outside_lanes(facts: &RepoFacts, sizes: &BTreeMap<RepoPath, u64>) -> Option<u64> {
    let globs = listing(facts, &facts.rust_roots)?
        .iter()
        .map(|pattern| listed_glob(pattern))
        .collect::<Option<Vec<PathGlob>>>()?;
    Some(
        sizes
            .iter()
            .filter(|(path, _)| !globs.iter().any(|glob| glob.matches(path)))
            .map(|(_, size)| size)
            .sum(),
    )
}

fn listed_glob(pattern: &str) -> Option<PathGlob> {
    let mut text = String::with_capacity(pattern.len());
    let mut chars = pattern.trim_start_matches('/').chars();
    while let Some(c) = chars.next() {
        text.push(if c == '\\' { chars.next()? } else { c });
    }
    PathGlob::new(text.trim_end_matches('/')).ok()
}
