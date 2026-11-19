// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod build;
mod freshness;
mod groups;
mod inventory;
mod options;
mod selection;
mod setup;

pub(crate) use build::build;
pub use options::AuditOptions;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, MemberDependency, RepoFacts, RepoPath};
use velnor_rust::{
    INPUTS_IDENTITY_HELPER, INPUTS_LOCK, Located, Owner, Owners, add_reads, detect,
    inputs_lock_text, locate, parse_inputs_lock, rerun_if_changed, unlisted,
};

use super::unit_platforms::platforms;
use super::units::{UnitFile, apart, profile};
use crate::coverage::{Built, Fingerprint, Unit, UnitKind};
use crate::error::CiError;
use crate::platform::Platforms;
use groups::{audit_inheriting, group_dirs, shared_groups, workspaces};
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
    let inherit = audit_inheriting(&facts, &checkout.root, directory, options);
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
    );
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

impl Audit<'_> {
    fn check_stale(&self) -> Result<(), CiError> {
        if self.stale.is_empty() {
            return Ok(());
        }
        Err(CiError::new(format!(
            "found stale dep-info {}; rebuild every observed target through velnor-ci inputs build -- cargo build --workspace --all-targets or delete its stale unit files before auditing inputs",
            self.stale.join(", ")
        )))
    }

    fn record(&mut self, indices: &[usize], file: &Path, unit: UnitFile) -> Result<(), CiError> {
        match unit {
            UnitFile::Fingerprint {
                configuration,
                package,
                hash,
                name,
            } => {
                for index in self.member_owners(&package, indices) {
                    self.fingerprint(
                        index,
                        configuration.clone(),
                        package.clone(),
                        hash.clone(),
                        &name,
                    );
                }
            }
            UnitFile::DepInfo {
                configuration,
                hash,
                stem,
            } => self.record_dep_info(
                indices,
                file,
                configuration.as_deref(),
                hash.as_deref(),
                &stem,
            )?,
            UnitFile::Script { profile, package } => {
                let owners = self.member_owners(&package, indices);
                if let Some(first) = owners.first() {
                    let base = self.roots[*first].clone();
                    self.script(&base, &owners, file, &profile, &package)?;
                }
            }
            UnitFile::BuildOutput {
                configuration,
                package,
                hash,
            } => {
                let owners = self.member_owners(&package, indices);
                if !owners.is_empty() {
                    self.build_output(&owners, file, &configuration, &package, &hash)?;
                }
            }
            UnitFile::Directory { name, root } => {
                for index in indices {
                    self.directory(*index, name.clone(), root);
                }
            }
        }
        Ok(())
    }

    fn record_dep_info(
        &mut self,
        indices: &[usize],
        file: &Path,
        configuration: Option<&str>,
        hash: Option<&str>,
        stem: &str,
    ) -> Result<(), CiError> {
        let mut hits: Vec<(usize, (String, Fingerprint))> = Vec::new();
        for index in indices {
            let key = configuration.map(|configuration| {
                let key = (
                    *index,
                    configuration.to_owned(),
                    hash.unwrap_or(stem).to_owned(),
                );
                self.present.insert(key.clone());
                key
            });
            if let Some(identity) = key.and_then(|key| match hash {
                Some(_) => self.fingerprints.get(&key),
                None => self.names.get(&key),
            }) {
                hits.push((*index, identity.clone()));
            }
        }
        if hash.is_none() && hits.is_empty() {
            return Ok(());
        }
        let budget = hits.first().map_or(indices[0], |(index, _)| *index);
        let base = self.roots[budget].clone();
        let package = hits.first().map(|(_, (package, _))| package.as_str());
        let Some((source, _)) = self.dep_info(&base, budget, file, package)? else {
            return Ok(());
        };
        if let Some(configuration) = configuration {
            for (index, (package, fingerprint)) in hits {
                self.built[index].units.insert(Unit {
                    configuration: configuration.to_owned(),
                    kind: fingerprint.kind,
                    package,
                    target: fingerprint.target,
                    source: source.clone(),
                });
            }
        }
        Ok(())
    }

    fn member_owners(&self, package: &str, indices: &[usize]) -> Vec<usize> {
        match self.members.get(package) {
            Some(owners) => indices
                .iter()
                .copied()
                .filter(|index| owners.contains(index))
                .collect(),
            None => indices.to_vec(),
        }
    }

    fn build_output(
        &mut self,
        indices: &[usize],
        file: &Path,
        configuration: &str,
        package: &str,
        hash: &str,
    ) -> Result<(), CiError> {
        let text = self.build_output_text(indices[0], file)?;
        let built = modified(file)?;
        let paths = rerun_if_changed(&text);
        let mut seen = BTreeSet::new();
        for &index in indices {
            let scripts: Vec<RepoPath> = self
                .scripts
                .get(&(index, profile(configuration).to_owned(), package.to_owned()))
                .into_iter()
                .flatten()
                .filter(|(_, compiled)| **compiled <= built)
                .map(|(source, _)| source.clone())
                .collect();
            let mut current = false;
            for source in scripts {
                let mut ran = false;
                for owner in self.owners.of(Some(package), &source) {
                    let base = owner.directory().map_or_else(
                        || self.repository.to_path_buf(),
                        |d| self.repository.join(d.as_str()),
                    );
                    if self.changed_since(&base, &paths, built, file, true)? {
                        continue;
                    }
                    ran = true;
                    if seen.insert((owner.manifest().clone(), owner.directory().cloned())) {
                        self.credit(&owner, &base, &paths, true);
                        self.units += 1;
                    }
                }
                if ran {
                    current = true;
                    self.built[index].units.insert(Unit {
                        configuration: configuration.to_owned(),
                        kind: UnitKind::Run,
                        package: package.to_owned(),
                        target: String::new(),
                        source,
                    });
                }
            }
            if current {
                self.present
                    .insert((index, configuration.to_owned(), hash.to_owned()));
            }
        }
        Ok(())
    }

    fn credit(&mut self, owner: &Owner<'_>, base: &Path, paths: &[String], declared: bool) {
        let mut reads = Vec::new();
        for path in paths {
            match locate(self.repository, base, path) {
                Located::Inside(read) => reads.push(Some(read)),
                Located::Repository => reads.push(None),
                Located::Outside => {}
            }
            if let Ok(Located::Inside(target)) =
                fs::canonicalize(base.join(path)).map(|target| locate(self.repository, &target, ""))
            {
                reads.push(Some(target));
            }
        }
        add_reads(
            &mut self.found,
            self.tracked,
            owner,
            &reads,
            declared,
            &self.targets,
        );
    }
}

fn modified(path: &Path) -> Result<SystemTime, CiError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))
}
