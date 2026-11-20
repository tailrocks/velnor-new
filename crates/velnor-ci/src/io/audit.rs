// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod build;
mod freshness;
mod groups;
mod inventory;
mod options;
mod record;
mod selection;
mod setup;
mod timing;

pub(crate) use build::build;
pub use options::AuditOptions;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, MemberDependency, RepoFacts, RepoPath};
use velnor_rust::{
    INPUTS_IDENTITY_HELPER, INPUTS_LOCK, Owners, detect, inputs_lock_text, parse_inputs_lock,
    unlisted,
};

use super::unit_platforms::platforms;
use super::units::apart;
use crate::coverage::{Built, Fingerprint};
use crate::error::CiError;
use crate::platform::Platforms;
use groups::{audit_inheriting, group_dirs, shared_groups, workspaces};
use record::modified;
use selection::{selected, within};

mod dep_info;

pub(super) struct Audit<'a> {
    repository: &'a Path,
    owners: Owners<'a>,
    tracked: &'a BTreeSet<RepoPath>,
    freshness: freshness::Freshness,
    pub(super) fingerprints: BTreeMap<(usize, String, String), (String, Fingerprint)>,
    pub(super) names: BTreeMap<(usize, String, String), (String, Fingerprint)>,
    pub(super) runs: BTreeMap<(usize, String, String), String>,
    pub(super) placed: BTreeSet<(usize, String)>,
    pub(super) rooted: BTreeSet<(usize, String)>,
    pub(super) present: BTreeSet<(usize, String, String)>,
    pub(super) scripts: BTreeMap<(usize, String, String), BTreeMap<RepoPath, SystemTime>>,
    targets: Vec<RepoPath>,
    roots: Vec<PathBuf>,
    members: BTreeMap<String, Vec<usize>>,
    found: CompileInputs,
    /// Bytes read from each Cargo root's target directory, bounded per root.
    target_bytes: Vec<u64>,
    stale: Vec<String>,
    units: usize,
    pub(super) built: Vec<Built>,
}

pub fn audit_inputs(directory: &Path, options: &AuditOptions) -> Result<String, CiError> {
    let failed = |e: &dyn std::fmt::Display| CiError::new(e.to_string());
    let mut checkout = read_checkout(directory).map_err(|e| failed(&e))?;
    let mut listed = checkout
        .view
        .files
        .remove(&INPUTS_LOCK)
        .map(|bytes| parse_inputs_lock(&bytes))
        .transpose()
        .map_err(|e| failed(&e))?
        .unwrap_or_default();
    let facts = detect(&checkout.view).map_err(|e| failed(&e))?;
    let inherit = audit_inheriting(&facts, &checkout.root, directory, options)?;
    let workspaces = workspaces(&facts, &checkout.root, &inherit)?;
    let shared = shared_groups(&facts);
    apart(&workspaces, &shared)?;
    let chosen = selected(&facts, &shared, options.root.as_deref())?;
    let groups = group_dirs(&facts, &checkout.root);
    let mut audit = setup::begin(
        &checkout,
        &facts,
        &workspaces,
        &groups,
        options.max_directory_entries,
    )?;
    for (unit, file, indices) in setup::gathered_files(&workspaces, &shared, &groups, &chosen)? {
        audit.record(&indices, &file, unit)?;
    }
    audit.orphans();
    let gated = gated(&facts);
    let platforms = workspaces
        .iter()
        .zip(&audit.built)
        .map(|(workspace, built)| {
            if gated {
                platforms(&workspace.workspace_root, built)
            } else {
                Ok(Platforms::default())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let effective: Vec<PathBuf> = (0..workspaces.len())
        .map(|index| {
            shared
                .iter()
                .zip(&groups)
                .find(|(indices, _)| indices.contains(&index))
                .map_or_else(
                    || workspaces[index].target_directory.clone(),
                    |(_, directory)| directory.clone(),
                )
        })
        .collect();
    setup::complete(&facts, &chosen, &effective, &audit.built, &platforms)?;
    audit.check_stale()?;
    let identified = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(INPUTS_IDENTITY_HELPER));
    let reads: usize = audit.found.values().map(BTreeSet::len).sum();
    let summary = format!("{reads} reads from {} unit files", audit.units);
    if options.write {
        let lock = checkout.root.join(INPUTS_LOCK.as_str());
        write_lock(&lock, &audit.found, identified)?;
        return Ok(format!("wrote {INPUTS_LOCK}: {summary}\n"));
    }
    if options.root.is_some() {
        listed = within(listed, &facts, &chosen);
    }
    compare(&audit.found, &listed, &summary, identified)
}

fn gated(facts: &RepoFacts) -> bool {
    let platformed = |dependencies: &[MemberDependency]| {
        dependencies
            .iter()
            .any(|dependency| dependency.platform.is_some())
    };
    facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .any(|member| platformed(&member.dependencies))
        || facts
            .unowned
            .values()
            .any(|package| platformed(&package.dependencies))
}

fn write_lock(lock: &Path, found: &CompileInputs, identified: bool) -> Result<(), CiError> {
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
    }
    fs::write(lock, inputs_lock_text(found, identified))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", lock.display())))
}

fn compare(
    found: &CompileInputs,
    listed: &CompileInputs,
    summary: &str,
    identified: bool,
) -> Result<String, CiError> {
    let missing = unlisted(found, listed);
    let unread = unlisted(listed, found);
    if missing.is_empty() && unread.is_empty() {
        return Ok(format!("{INPUTS_LOCK} lists every read: {summary}\n"));
    }
    let sections: Vec<String> = [
        ("reads it does not list", missing),
        ("lines nothing reads", unread),
    ]
    .into_iter()
    .filter(|(_, entries)| !entries.is_empty())
    .map(|(heading, entries)| format!("{heading}:\n{}", inputs_lock_text(&entries, identified)))
    .collect();
    Err(CiError::new(format!(
        "{INPUTS_LOCK} is stale; run velnor-ci inputs build -- cargo build --workspace --all-targets, then velnor-ci inputs audit --write and commit it\n{}",
        sections.concat().trim_end()
    )))
}
