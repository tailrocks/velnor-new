// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::Deserialize;
use velnor_gen::io::read_checkout;
use velnor_model::{CompileInputs, RepoFacts, RepoPath};
use velnor_rust::{
    INPUTS_LOCK, Located, Owner, add_reads, build_script_owners, dep_info_paths, detect,
    format_inputs_lock, locate, parse_inputs_lock, rerun_if_changed, source_owners, unlisted,
};

use crate::error::CiError;

#[derive(Deserialize)]
struct Workspace {
    workspace_root: PathBuf,
    target_directory: PathBuf,
}

enum UnitFile {
    DepInfo,
    BuildOutput(String),
}

struct Audit<'a> {
    repository: &'a Path,
    facts: &'a RepoFacts,
    tracked: &'a BTreeSet<RepoPath>,
    targets: Vec<RepoPath>,
    found: CompileInputs,
    units: usize,
    built: BTreeSet<RepoPath>,
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
    let mut audit = Audit {
        repository: &checkout.root,
        facts: &facts,
        tracked: &checkout.view.tracked_paths,
        targets: workspaces
            .iter()
            .filter_map(|w| match locate(&checkout.root, &w.target_directory, "") {
                Located::Inside(target) => Some(target),
                Located::Repository | Located::Outside => None,
            })
            .collect(),
        found: CompileInputs::new(),
        units: 0,
        built: BTreeSet::new(),
    };
    for workspace in &workspaces {
        let mut files = Vec::new();
        unit_files(&workspace.target_directory, &mut files)?;
        for (file, kind) in files {
            audit.record(&workspace.workspace_root, &file, &kind)?;
        }
    }
    let targets: Vec<String> = workspaces
        .iter()
        .map(|w| w.target_directory.display().to_string())
        .collect();
    let unbuilt: Vec<String> = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .filter(|member| !audit.built.contains(&member.manifest))
        .map(|member| format!("{} ({})", member.name, member.manifest))
        .collect();
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
        let declared = matches!(kind, UnitFile::BuildOutput(_));
        let readers: Vec<(Owner<'_>, PathBuf, Vec<String>)> = match kind {
            UnitFile::DepInfo => {
                let paths = dep_info_paths(&text);
                let owners = match paths
                    .first()
                    .map(|source| locate(self.repository, workspace, source))
                {
                    Some(Located::Inside(source)) => source_owners(self.facts, &source),
                    _ => Vec::new(),
                };
                owners
                    .into_iter()
                    .map(|owner| (owner, workspace.to_path_buf(), paths.clone()))
                    .collect()
            }
            UnitFile::BuildOutput(package) => build_script_owners(self.facts, package)
                .into_iter()
                .map(|owner| {
                    let base = owner.directory().map_or_else(
                        || self.repository.to_path_buf(),
                        |d| self.repository.join(d.as_str()),
                    );
                    (owner, base, rerun_if_changed(&text))
                })
                .collect(),
        };
        let built = modified(file)?;
        if !readers.is_empty() {
            self.units += 1;
        }
        for (owner, base, paths) in readers {
            if paths
                .iter()
                .any(|path| modified(&base.join(path)).is_ok_and(|time| time > built))
            {
                continue;
            }
            let mut reads = Vec::new();
            for path in &paths {
                match locate(self.repository, &base, path) {
                    Located::Inside(read) => reads.push(read),
                    Located::Repository => reads.extend(self.tracked.iter().cloned()),
                    Located::Outside => {}
                }
                if let Ok(Located::Inside(target)) = fs::canonicalize(base.join(path))
                    .map(|target| locate(self.repository, &target, ""))
                {
                    reads.push(target);
                }
            }
            reads.retain(|read| {
                !self.targets.iter().any(|target| {
                    read == target || read.as_str().starts_with(&format!("{target}/"))
                })
            });
            add_reads(&mut self.found, self.tracked, &owner, &reads, declared);
            self.built.insert(owner.manifest().clone());
        }
        Ok(())
    }
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
    if name.strip_suffix(".d").and_then(unit_name).is_some() {
        return Some(UnitFile::DepInfo);
    }
    let unit = path.parent()?;
    let package = unit_name(unit.file_name()?.to_str()?)?;
    let build = unit.parent()?.file_name()?;
    (name == "output" && build == "build").then(|| UnitFile::BuildOutput(package.to_owned()))
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
