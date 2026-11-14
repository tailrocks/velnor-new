// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, PackageName, RepoFacts, RepoPath};
use velnor_rust::{
    INPUTS_LOCK, Located, Owner, Owners, add_reads, dep_info_paths, detect, format_inputs_lock,
    locate, parse_inputs_lock, rerun_if_changed, unlisted,
};

use super::units::{UnitFile, Workspace, apart, profile, triples, unit_files, workspace};
use crate::coverage::{Built, Fingerprint, HOST, Unit, UnitKind, gaps};
use crate::error::CiError;

struct Audit<'a> {
    repository: &'a Path,
    owners: Owners<'a>,
    tracked: &'a BTreeSet<RepoPath>,
    fingerprints: BTreeMap<(usize, String, String), (String, Fingerprint)>,
    names: BTreeMap<(usize, String, String), (String, Fingerprint)>,
    present: BTreeSet<(usize, String, String)>,
    scripts: BTreeMap<(usize, String, String), BTreeMap<RepoPath, SystemTime>>,
    targets: Vec<RepoPath>,
    found: CompileInputs,
    units: usize,
    built: Vec<Built>,
}

pub fn audit_inputs(directory: &Path, write: bool) -> Result<String, CiError> {
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
        .map(|root| workspace(&checkout.root.join(root.manifest.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    apart(&workspaces)?;
    let mut audit = Audit {
        repository: &checkout.root,
        owners: Owners::new(&facts, &checkout.view.tracked_paths),
        tracked: &checkout.view.tracked_paths,
        fingerprints: BTreeMap::new(),
        names: BTreeMap::new(),
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
    let files_under_triples = files
        .iter()
        .any(|(unit, _, _)| unit.configuration().is_some_and(|c| c.contains('/')));
    for (unit, file, index) in files {
        audit.record(&workspaces[index].workspace_root, index, &file, unit)?;
    }
    audit.orphans();
    let triples = if files_under_triples {
        triples(&checkout.root)?
    } else {
        BTreeSet::new()
    };
    complete(&facts, &workspaces, &audit.built, &triples)?;
    let reads: usize = audit.found.values().map(BTreeSet::len).sum();
    let summary = format!("{reads} reads from {} unit files", audit.units);
    if write {
        write_lock(&checkout.root.join(INPUTS_LOCK.as_str()), &audit.found)?;
        return Ok(format!("wrote {INPUTS_LOCK}: {summary}\n"));
    }
    compare(&audit.found, &listed, &summary)
}

fn complete(
    facts: &RepoFacts,
    workspaces: &[Workspace],
    built: &[Built],
    triples: &BTreeSet<String>,
) -> Result<(), CiError> {
    let gaps: Vec<String> = facts
        .rust_roots
        .iter()
        .zip(workspaces)
        .zip(built)
        .flat_map(|((root, workspace), built)| {
            gaps(root, &facts.unowned, built, triples)
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
        "found no current dep-info {}; run this after cargo build --all-targets in every Cargo root, and build fully or delete each debug directory that holds a test harness of a member",
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
        "{INPUTS_LOCK} is stale; run velnor-ci inputs audit --write after a build of all targets and commit it\n{}",
        sections.concat().trim_end()
    )))
}

impl Audit<'_> {
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
            } => {
                if let Some(fingerprint) = Fingerprint::parse(&name) {
                    if fingerprint.kind == UnitKind::Normal {
                        self.names.insert(
                            (index, configuration.clone(), fingerprint.target.clone()),
                            (package.clone(), fingerprint.clone()),
                        );
                    }
                    self.fingerprints.insert(
                        (index, configuration.clone(), hash),
                        (package.clone(), fingerprint),
                    );
                }
                if let Ok(package) = PackageName::new(package) {
                    if name.starts_with("lib-") {
                        self.built[index].libraries.insert((configuration, package));
                    } else if name.starts_with("build-script-") && !name.contains('.') {
                        self.built[index].builders.insert((configuration, package));
                    }
                }
            }
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
                let Some((source, _)) = self.dep_info(workspace, file)? else {
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
            } => self.build_output(index, file, &configuration, &package)?,
        }
        Ok(())
    }

    fn script(
        &mut self,
        workspace: &Path,
        index: usize,
        file: &Path,
        profile: String,
        package: String,
    ) -> Result<(), CiError> {
        if let Some((source, built)) = self.dep_info(workspace, file)? {
            if profile == HOST {
                self.built[index]
                    .scripts
                    .insert((package.clone(), source.clone()));
            }
            self.scripts
                .entry((index, profile, package))
                .or_default()
                .entry(source)
                .and_modify(|compiled| *compiled = (*compiled).min(built))
                .or_insert(built);
        }
        Ok(())
    }

    fn orphans(&mut self) {
        for ((index, configuration, hash), (package, fingerprint)) in &self.fingerprints {
            let key = |name: &String| (*index, configuration.clone(), name.clone());
            if !self.present.contains(&key(hash))
                && !self.present.contains(&key(&fingerprint.target))
            {
                self.built[*index].orphans.insert((
                    configuration.clone(),
                    package.clone(),
                    fingerprint.clone(),
                ));
            }
        }
    }

    fn dep_info(
        &mut self,
        workspace: &Path,
        file: &Path,
    ) -> Result<Option<(RepoPath, SystemTime)>, CiError> {
        let text = read(file)?;
        let built = modified(file)?;
        let paths = dep_info_paths(&text);
        let Some(Located::Inside(source)) = paths
            .first()
            .map(|source| locate(self.repository, workspace, source))
        else {
            return Ok(None);
        };
        if changed_since(workspace, &paths, built) {
            return Ok(None);
        }
        for owner in self.owners.of(&source) {
            self.credit(&owner, workspace, &paths, false);
        }
        self.units += 1;
        Ok(Some((source, built)))
    }

    fn build_output(
        &mut self,
        index: usize,
        file: &Path,
        configuration: &str,
        package: &str,
    ) -> Result<(), CiError> {
        let text = read(file)?;
        let built = modified(file)?;
        let paths = rerun_if_changed(&text);
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
            for owner in self.owners.of(&source) {
                let base = owner.directory().map_or_else(
                    || self.repository.to_path_buf(),
                    |d| self.repository.join(d.as_str()),
                );
                if changed_since(&base, &paths, built) {
                    continue;
                }
                ran = true;
                if seen.insert((owner.manifest().clone(), owner.directory().cloned())) {
                    self.credit(&owner, &base, &paths, true);
                    self.units += 1;
                }
            }
            if ran {
                self.built[index].units.insert(Unit {
                    configuration: configuration.to_owned(),
                    kind: UnitKind::Run,
                    package: package.to_owned(),
                    target: String::new(),
                    source,
                });
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

fn changed_since(base: &Path, paths: &[String], built: SystemTime) -> bool {
    paths
        .iter()
        .any(|path| modified(&base.join(path)).is_ok_and(|time| time > built))
}

fn read(file: &Path) -> Result<String, CiError> {
    fs::read_to_string(file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))
}

fn modified(path: &Path) -> Result<SystemTime, CiError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))
}
