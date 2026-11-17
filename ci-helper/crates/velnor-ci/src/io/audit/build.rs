// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod declarations;
mod invocation;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::RepoFacts;
use velnor_rust::{detect, sha256_hex};

use super::super::units::{UnitFile, Workspace, apart, unit_files, workspace};
use super::inventory::{self, Entries};
use crate::cli::InputsBuildArgs;
use crate::error::CiError;
use invocation::Invocation;

struct Builder<'a> {
    directory: &'a Path,
    repository: PathBuf,
    facts: RepoFacts,
    workspaces: Vec<Workspace>,
    targets: BTreeSet<PathBuf>,
    roots: BTreeSet<PathBuf>,
    location: PathBuf,
}

pub(crate) fn build(directory: &Path, args: &InputsBuildArgs) -> Result<String, CiError> {
    let checkout = read_checkout(directory).map_err(|e| CiError::new(e.to_string()))?;
    let facts = detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
    let workspaces = facts
        .rust_roots
        .iter()
        .map(|root| workspace(&checkout.root.join(root.manifest.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    apart(&workspaces)?;
    let invocation = Invocation::new(&args.command);
    let mut roots = BTreeSet::from([checkout.root.clone()]);
    roots.extend(args.directory.iter().map(|root| directory.join(root)));
    let mut builder = Builder {
        directory,
        repository: checkout.root,
        facts,
        targets: inventory::exclusions(&workspaces),
        workspaces,
        roots,
        location: fs::canonicalize(invocation.location(directory))
            .map_err(|e| CiError::new(e.to_string()))?,
    };
    if let Some(preflight) = &invocation.preflight {
        builder.stabilize(preflight, true, true)?;
    }
    builder.stabilize(
        &invocation.command,
        invocation.compiler,
        invocation.cargo && invocation.preflight.is_none(),
    )?;
    Ok(String::new())
}

impl Builder<'_> {
    fn declared(&self) -> Result<Vec<declarations::Declared>, CiError> {
        declarations::declarations(
            &self.repository,
            &self.facts,
            &self.workspaces,
            &self.location,
        )
    }

    fn stabilize(
        &mut self,
        command: &[String],
        replayable: bool,
        invalidate: bool,
    ) -> Result<(), CiError> {
        for _ in 0..if replayable { 2 } else { 1 } {
            let declared = self.declared()?;
            self.roots.extend(
                declared
                    .iter()
                    .flat_map(|unit| unit.directories.iter().cloned()),
            );
            if invalidate {
                let invocation = Invocation::new(command);
                if declared
                    .iter()
                    .any(|unit| !unit.trusted && !invocation.selects(&unit.package))
                {
                    return Err(CiError::new(
                        "untrusted build outputs include packages outside this command's explicit selection; prepare inventory first with velnor-ci inputs build -- cargo build --workspace --all-targets, preserving this build's feature, target and profile options",
                    ));
                }
                declarations::invalidate(&declared)?;
            }
            let (entries, complete) = self.capture();
            self.run(command, &entries, complete)?;
            if !replayable || self.declared()?.iter().all(|unit| unit.trusted) {
                return Ok(());
            }
        }
        Err(CiError::new(
            "build directory declarations did not stabilize after two compiler passes; a compiler pass must leave declared directory membership unchanged before inputs can be audited",
        ))
    }

    fn capture(&self) -> (Entries, bool) {
        let mut entries = Entries::new();
        let mut remaining = inventory::LIMIT * 10;
        for root in &self.roots {
            if !root.exists() {
                continue;
            }
            match inventory::scan(root, &self.targets, &mut remaining) {
                Ok(found) => entries.extend(found),
                Err(_) => return (entries, false),
            }
        }
        (entries, true)
    }

    fn run(&self, command: &[String], entries: &Entries, complete: bool) -> Result<(), CiError> {
        let mut before = BTreeMap::new();
        for workspace in &self.workspaces {
            for (unit, path) in unit_files(&workspace.target_directory)? {
                if output(&unit) {
                    before.insert(path.clone(), identity(&path)?);
                }
            }
        }
        let started = SystemTime::now();
        let status = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(self.directory)
            .status()
            .map_err(|e| CiError::new(format!("cannot run build command: {e}")))?;
        self.record(entries, complete, &before, started, status)?;
        if !status.success() {
            return Err(CiError::new(format!("build command failed: {status}")));
        }
        Ok(())
    }

    fn record(
        &self,
        entries: &Entries,
        complete: bool,
        before: &BTreeMap<PathBuf, (String, SystemTime)>,
        started: SystemTime,
        status: ExitStatus,
    ) -> Result<(), CiError> {
        let bytes = serde_json::to_vec(entries).map_err(|e| CiError::new(e.to_string()))?;
        let digest = sha256_hex(&bytes);
        for workspace in &self.workspaces {
            let store = workspace.target_directory.join(inventory::STORE);
            let saved = store.join(format!("{digest}.json"));
            if complete && status.success() {
                fs::create_dir_all(&store)
                    .and_then(|()| fs::write(&saved, &bytes))
                    .map_err(|e| CiError::new(format!("cannot save input inventory: {e}")))?;
            }
            for (unit, file) in unit_files(&workspace.target_directory)? {
                if !output(&unit) {
                    continue;
                }
                let current = identity(&file)?;
                if before.get(&file) == Some(&current) {
                    continue;
                }
                let receipt = inventory::receipt_path(&file);
                if complete && status.success() && current.1 >= started {
                    let record = inventory::receipt(&file, saved.clone(), digest.clone())
                        .map_err(|e| CiError::new(e.to_string()))?;
                    let text =
                        serde_json::to_vec(&record).map_err(|e| CiError::new(e.to_string()))?;
                    fs::write(&receipt, text).map_err(|e| {
                        CiError::new(format!("cannot save {}: {e}", receipt.display()))
                    })?;
                } else if receipt.exists() {
                    fs::remove_file(&receipt).map_err(|e| {
                        CiError::new(format!("cannot invalidate {}: {e}", receipt.display()))
                    })?;
                }
            }
        }
        Ok(())
    }
}

fn output(unit: &UnitFile) -> bool {
    matches!(
        unit,
        UnitFile::BuildOutput { .. } | UnitFile::DepInfo { .. } | UnitFile::Script { .. }
    )
}

fn identity(path: &Path) -> Result<(String, SystemTime), CiError> {
    let bytes = fs::read(path).map_err(|e| CiError::new(e.to_string()))?;
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(e.to_string()))?;
    Ok((sha256_hex(&bytes), modified))
}
