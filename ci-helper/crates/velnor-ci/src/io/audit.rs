// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod build;
mod freshness;
mod inventory;
mod options;

pub(crate) use build::build;
pub use options::AuditOptions;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, MemberDependency, RepoFacts, RepoPath};
use velnor_rust::{
    INPUTS_LOCK, Located, Owner, Owners, add_reads, detect, format_inputs_lock, locate,
    parse_inputs_lock, rerun_if_changed, unlisted,
};

use super::units::{UnitFile, Workspace, apart, platforms, profile, unit_files, workspace};
use crate::coverage::{Built, Fingerprint, Unit, UnitKind, gaps};
use crate::error::CiError;
use crate::platform::Platforms;

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
    let listed = checkout
        .view
        .files
        .remove(&INPUTS_LOCK)
        .map(|bytes| parse_inputs_lock(&bytes))
        .transpose()
        .map_err(|e| failed(&e))?
        .unwrap_or_default();
    let facts = detect(&checkout.view).map_err(|e| failed(&e))?;
    let workspaces = facts
        .rust_roots
        .iter()
        .map(|root| workspace(&checkout.root.join(root.manifest.as_str()), None))
        .collect::<Result<Vec<_>, _>>()?;
    apart(&workspaces)?;
    let mut audit = Audit {
        repository: &checkout.root,
        owners: Owners::new(&facts, &checkout.view.tracked_paths),
        tracked: &checkout.view.tracked_paths,
        freshness: freshness::Freshness::new(&workspaces, options.max_directory_entries),
        fingerprints: BTreeMap::new(),
        names: BTreeMap::new(),
        runs: BTreeMap::new(),
        placed: BTreeSet::new(),
        rooted: BTreeSet::new(),
        present: BTreeSet::new(),
        scripts: BTreeMap::new(),
        targets: workspaces
            .iter()
            .filter_map(|w| match locate(&checkout.root, &w.target_directory, "") {
                Located::Inside(target) => Some(target),
                Located::Repository | Located::Outside => None,
            })
            .collect(),
        found: CompileInputs::new(),
        target_bytes: vec![0; workspaces.len()],
        stale: Vec::new(),
        units: 0,
        built: workspaces.iter().map(|_| Built::default()).collect(),
    };
    let mut files = Vec::new();
    for (index, workspace) in workspaces.iter().enumerate() {
        files.extend(
            unit_files(&workspace.target_directory)?
                .into_iter()
                .map(|(unit, file)| (unit, file, index)),
        );
    }
    files.sort();
    for (unit, file, index) in files {
        audit.record(&workspaces[index].workspace_root, index, &file, unit)?;
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
    complete(&facts, &workspaces, &audit.built, &platforms)?;
    audit.check_stale()?;
    let reads: usize = audit.found.values().map(BTreeSet::len).sum();
    let summary = format!("{reads} reads from {} unit files", audit.units);
    if options.write {
        write_lock(&checkout.root.join(INPUTS_LOCK.as_str()), &audit.found)?;
        return Ok(format!("wrote {INPUTS_LOCK}: {summary}\n"));
    }
    compare(&audit.found, &listed, &summary)
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

fn complete(
    facts: &RepoFacts,
    workspaces: &[Workspace],
    built: &[Built],
    platforms: &[Platforms],
) -> Result<(), CiError> {
    let gaps: Vec<String> = facts
        .rust_roots
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

fn write_lock(lock: &Path, found: &CompileInputs) -> Result<(), CiError> {
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
    }
    fs::write(lock, format_inputs_lock(found))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", lock.display())))
}

fn compare(
    found: &CompileInputs,
    listed: &CompileInputs,
    summary: &str,
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
    .map(|(heading, entries)| format!("{heading}:\n{}", format_inputs_lock(&entries)))
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

    fn record(
        &mut self,
        workspace: &Path,
        index: usize,
        file: &Path,
        unit: UnitFile,
    ) -> Result<(), CiError> {
        match unit {
            UnitFile::Fingerprint {
                configuration,
                package,
                hash,
                name,
            } => self.fingerprint(index, configuration, package, hash, &name),
            UnitFile::DepInfo {
                configuration,
                hash,
                stem,
            } => {
                let identity = configuration.as_ref().and_then(|configuration| {
                    let key = (
                        index,
                        configuration.clone(),
                        hash.clone().unwrap_or_else(|| stem.clone()),
                    );
                    self.present.insert(key.clone());
                    match hash {
                        Some(_) => self.fingerprints.get(&key),
                        None => self.names.get(&key),
                    }
                    .cloned()
                });
                if hash.is_none() && identity.is_none() {
                    return Ok(());
                }
                let package = identity.as_ref().map(|(package, _)| package.as_str());
                let Some((source, _)) = self.dep_info(workspace, index, file, package)? else {
                    return Ok(());
                };
                if let (Some(configuration), Some((package, fingerprint))) =
                    (configuration, identity)
                {
                    self.built[index].units.insert(Unit {
                        configuration,
                        kind: fingerprint.kind,
                        package,
                        target: fingerprint.target,
                        source,
                    });
                }
            }
            UnitFile::Script { profile, package } => {
                self.script(workspace, index, file, profile, package)?;
            }
            UnitFile::BuildOutput {
                configuration,
                package,
                hash,
            } => {
                if self.build_output(index, file, &configuration, &package)? {
                    self.present.insert((index, configuration, hash));
                }
            }
            UnitFile::Directory { name, root } => self.directory(index, name, root),
        }
        Ok(())
    }

    fn build_output(
        &mut self,
        index: usize,
        file: &Path,
        configuration: &str,
        package: &str,
    ) -> Result<bool, CiError> {
        let text = self.build_output_text(index, file)?;
        let built = modified(file)?;
        let paths = rerun_if_changed(&text);
        let mut current = false;
        let scripts: Vec<RepoPath> = self
            .scripts
            .get(&(index, profile(configuration).to_owned(), package.to_owned()))
            .into_iter()
            .flatten()
            .filter(|(_, compiled)| **compiled <= built)
            .map(|(source, _)| source.clone())
            .collect();
        let mut seen = BTreeSet::new();
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
        Ok(current)
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
