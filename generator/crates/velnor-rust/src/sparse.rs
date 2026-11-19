// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CheckoutScope, RepoFacts, RepoPath, RustRoot};

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

pub(crate) fn paths(facts: &RepoFacts, roots: &[RustRoot]) -> Option<Vec<String>> {
    if facts.config.checkout == CheckoutScope::Full
        || roots.is_empty()
        || !facts.config.test_setup.is_empty()
        || facts.compile_inputs.contains_key(&None)
    {
        return None;
    }
    let mut paths: BTreeSet<String> = TOP_FILES.iter().map(|file| format!("/{file}")).collect();
    paths.extend(TOP_DIRECTORIES.iter().map(|dir| format!("/{dir}/")));
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
    }
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
            .map(|rule| format!("/{}", rule.glob)),
    );
    Some(paths.into_iter().collect())
}

fn directory(manifest: &RepoPath) -> Option<String> {
    let text = manifest.as_str();
    match text.strip_suffix("Cargo.toml") {
        None => Some(format!("/{text}/")),
        Some("") => None,
        Some(parent) => Some(format!("/{parent}")),
    }
}
