// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CompileInputs, PackageName, RepoFacts, RustRoot};

use super::super::units::Workspace;
use crate::coverage::{Built, gaps};
use crate::error::CiError;
use crate::platform::Platforms;

/// The lock lines of packages that are members of the selected roots.
pub(super) fn within(listed: CompileInputs, roots: &[&RustRoot]) -> CompileInputs {
    let members: BTreeSet<&PackageName> = roots
        .iter()
        .flat_map(|root| &root.members)
        .map(|member| &member.name)
        .collect();
    listed
        .into_iter()
        .filter_map(|(path, packages)| {
            let packages: BTreeSet<PackageName> = packages
                .into_iter()
                .filter(|package| members.contains(package))
                .collect();
            (!packages.is_empty()).then_some((path, packages))
        })
        .collect()
}

/// Every Cargo root, or with `--root` only the one whose manifest sits in that directory.
pub(super) fn selected_roots<'a>(
    facts: &'a RepoFacts,
    directory: Option<&str>,
) -> Result<Vec<&'a RustRoot>, CiError> {
    let Some(directory) = directory else {
        return Ok(facts.rust_roots.iter().collect());
    };
    let wanted = directory.trim_end_matches('/');
    let wanted = wanted.strip_prefix("./").unwrap_or(wanted);
    let wanted = if wanted.is_empty() { "." } else { wanted };
    let directory_of = |root: &RustRoot| {
        root.manifest
            .parent()
            .map_or_else(|| ".".to_owned(), |parent| parent.as_str().to_owned())
    };
    let found: Vec<&RustRoot> = facts
        .rust_roots
        .iter()
        .filter(|root| directory_of(root) == wanted)
        .collect();
    if found.is_empty() {
        let known: Vec<String> = facts.rust_roots.iter().map(directory_of).collect();
        return Err(CiError::new(format!(
            "no Cargo root in {wanted}; the roots are {}",
            known.join(", ")
        )));
    }
    Ok(found)
}

pub(super) fn complete(
    facts: &RepoFacts,
    roots: &[&RustRoot],
    workspaces: &[Workspace],
    built: &[Built],
    platforms: &[Platforms],
) -> Result<(), CiError> {
    let gaps: Vec<String> = roots
        .iter()
        .zip(workspaces)
        .zip(built)
        .zip(platforms)
        .flat_map(|(((root, workspace), built), platform)| {
            gaps(root, &facts.unowned, built, platform)
                .into_iter()
                .map(|gap| {
                    format!(
                        "in {} for {}",
                        workspace
                            .target_directory
                            .join(&gap.configuration)
                            .display(),
                        gap.missing.join(", ")
                    )
                })
        })
        .collect();
    if gaps.is_empty() {
        return Ok(());
    }
    Err(CiError::new(format!(
        "found no current dep-info {}; run this after velnor-ci inputs build -- cargo build --workspace --all-targets in every Cargo root, and build fully or delete each debug directory that holds a test harness of a member",
        gaps.join("; ")
    )))
}
