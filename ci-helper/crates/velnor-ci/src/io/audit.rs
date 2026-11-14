// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::Deserialize;
use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, RepoFacts, RepoPath, TargetKind};
use velnor_rust::{
    INPUTS_LOCK, Located, Owner, Owners, add_reads, dep_info_paths, detect, format_inputs_lock,
    locate, normalize, parse_inputs_lock, rerun_if_changed, unlisted,
};

use crate::error::CiError;

#[derive(Deserialize)]
struct Workspace {
    workspace_root: PathBuf,
    target_directory: PathBuf,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum UnitFile {
    DepInfo(Option<String>),
    BuildOutput(String),
}

struct Audit<'a> {
    repository: &'a Path,
    owners: Owners<'a>,
    tracked: &'a BTreeSet<RepoPath>,
    scripts: BTreeMap<String, BTreeMap<RepoPath, SystemTime>>,
    targets: Vec<RepoPath>,
    found: CompileInputs,
    units: usize,
    covered: BTreeMap<RepoPath, usize>,
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
    let mut directories = BTreeSet::new();
    if let Some(shared) = workspaces
        .iter()
        .find(|w| !directories.insert(normalize(&w.target_directory)))
    {
        return Err(CiError::new(format!(
            "two Cargo roots share the target directory {}, so its units cannot be told apart; give each Cargo root its own target directory",
            shared.target_directory.display()
        )));
    }
    let mut audit = Audit {
        repository: &checkout.root,
        owners: Owners::new(&facts, &checkout.view.tracked_paths),
        tracked: &checkout.view.tracked_paths,
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
        covered: BTreeMap::new(),
    };
    let mut files = Vec::new();
    for workspace in &workspaces {
        let mut found = Vec::new();
        unit_files(&workspace.target_directory, &mut found)?;
        files.extend(
            found
                .into_iter()
                .map(|(file, kind)| (kind, file, &workspace.workspace_root)),
        );
    }
    files.sort();
    for (kind, file, workspace) in &files {
        audit.record(workspace, file, kind)?;
    }
    let targets: Vec<String> = workspaces
        .iter()
        .map(|w| w.target_directory.display().to_string())
        .collect();
    let unbuilt = uncovered(&facts, &audit.covered);
    if !unbuilt.is_empty() {
        return Err(CiError::new(format!(
            "found no current dep-info in {} for {}; run this after cargo build --all-targets in every Cargo root",
            targets.join(", "),
            unbuilt.join(", ")
        )));
    }
    let reads: usize = audit.found.values().map(BTreeSet::len).sum();
    let summary = format!("{reads} reads from {} unit files", audit.units);
    if write {
        write_lock(&checkout.root.join(INPUTS_LOCK.as_str()), &audit.found)?;
        return Ok(format!("wrote {INPUTS_LOCK}: {summary}\n"));
    }
    compare(&audit.found, &listed, &summary)
}

fn uncovered(facts: &RepoFacts, covered: &BTreeMap<RepoPath, usize>) -> Vec<String> {
    facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| {
            member
                .targets
                .iter()
                .filter(|target| !target.requires_features)
                .filter_map(|target| {
                    let source = target.source.as_ref()?;
                    let harness = target.tests
                        && target.kinds.iter().any(|kind| {
                            matches!(
                                kind,
                                TargetKind::Lib | TargetKind::ProcMacro | TargetKind::Bin
                            )
                        });
                    let expected = 1 + usize::from(harness);
                    let current = covered.get(source).copied().unwrap_or_default();
                    (current < expected).then(|| {
                        format!(
                            "{} ({}) target {} ({source}, {current} of {expected} units)",
                            member.name, member.manifest, target.name
                        )
                    })
                })
        })
        .collect()
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
    fn record(&mut self, workspace: &Path, file: &Path, kind: &UnitFile) -> Result<(), CiError> {
        let text = fs::read_to_string(file)
            .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
        let built = modified(file)?;
        match kind {
            UnitFile::DepInfo(script) => {
                let paths = dep_info_paths(&text);
                let Some(Located::Inside(source)) = paths
                    .first()
                    .map(|source| locate(self.repository, workspace, source))
                else {
                    return Ok(());
                };
                if changed_since(workspace, &paths, built) {
                    return Ok(());
                }
                match script {
                    Some(package) => {
                        self.scripts
                            .entry(package.clone())
                            .or_default()
                            .insert(source.clone(), built);
                    }
                    None => *self.covered.entry(source.clone()).or_default() += 1,
                }
                for owner in self.owners.of(&source) {
                    self.credit(&owner, workspace, &paths, false);
                }
                self.units += 1;
            }
            UnitFile::BuildOutput(package) => {
                let paths = rerun_if_changed(&text);
                let scripts: Vec<RepoPath> = self
                    .scripts
                    .get(package)
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
                        *self.covered.entry(source).or_default() += 1;
                    }
                }
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

fn workspace(manifest: &Path) -> Result<Workspace, CiError> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(manifest.parent().unwrap_or(manifest))
        .output()
        .map_err(|e| CiError::new(format!("cannot run cargo metadata: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| CiError::new(format!("cargo metadata printed no workspace: {e}")))
}

fn unit_files(directory: &Path, found: &mut Vec<(PathBuf, UnitFile)>) -> Result<(), CiError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CiError::new(format!(
                "cannot read {}: {e}",
                directory.display()
            )));
        }
    };
    for entry in entries {
        let entry =
            entry.map_err(|e| CiError::new(format!("cannot read {}: {e}", directory.display())))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
        if kind.is_dir() && entry.file_name() != "incremental" {
            unit_files(&path, found)?;
        } else if let Some(unit) = unit_file(&path).filter(|_| kind.is_file()) {
            found.push((path, unit));
        }
    }
    Ok(())
}

fn unit_file(path: &Path) -> Option<UnitFile> {
    let name = path.file_name()?.to_str()?;
    let unit = path.parent()?;
    let package = unit
        .file_name()
        .and_then(|folder| unit_name(folder.to_str()?))
        .filter(|_| unit.parent().and_then(Path::file_name) == Some("build".as_ref()));
    if name.strip_suffix(".d").and_then(unit_name).is_some() {
        return Some(UnitFile::DepInfo(package.map(str::to_owned)));
    }
    (name == "output")
        .then_some(package)?
        .map(|package| UnitFile::BuildOutput(package.to_owned()))
}

fn unit_name(stem: &str) -> Option<&str> {
    let (name, hash) = stem.rsplit_once('-')?;
    (!name.is_empty()
        && hash.len() == 16
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
    .then_some(name)
}

fn modified(path: &Path) -> Result<SystemTime, CiError> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))
}
