// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod clock;
mod declarations;
mod failure;
mod generated;
mod invocation;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_model::RepoFacts;
use velnor_rust::{detect, sha256_hex};

use super::super::units::{UnitFile, Workspace, apart, unit_files};
use super::groups::{group_dirs, inheriting, scan_dirs, shared_groups, standalone, workspaces};
use super::inventory::{self, Evidence};
use super::timing;
use crate::cli::InputsBuildArgs;
use crate::error::CiError;
use invocation::Invocation;

const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";

struct Builder<'a> {
    directory: &'a Path,
    repository: PathBuf,
    facts: RepoFacts,
    workspaces: Vec<Workspace>,
    directories: Vec<PathBuf>,
    targets: BTreeSet<PathBuf>,
    roots: BTreeSet<PathBuf>,
    declared: BTreeSet<PathBuf>,
    location: PathBuf,
}

type Identities = BTreeMap<PathBuf, (String, SystemTime)>;

pub(crate) fn build(directory: &Path, args: &InputsBuildArgs) -> Result<String, CiError> {
    let checkout = read_checkout(directory).map_err(|e| CiError::new(e.to_string()))?;
    let facts = detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
    let standalone = standalone(&facts);
    let invocation = Invocation::new(&args.command);
    let location = fs::canonicalize(invocation.location(directory))
        .map_err(|e| CiError::new(e.to_string()))?;
    let inherit = inheriting(&facts, &checkout.root, Some(&location))?;
    let workspaces = workspaces(&facts, &checkout.root, &inherit)?;
    apart(&workspaces, &shared_groups(&facts))?;
    let directories = scan_dirs(&workspaces);
    invocation.validate_targets(
        directory,
        &workspaces,
        &standalone,
        &inherit,
        &group_dirs(&facts, &checkout.root),
    )?;
    let mut roots = BTreeSet::from([checkout.root.clone()]);
    roots.extend(args.directory.iter().map(|root| directory.join(root)));
    let mut builder = Builder {
        directory,
        repository: checkout.root,
        facts,
        targets: inventory::exclusions(&workspaces)?,
        workspaces,
        directories,
        roots,
        declared: BTreeSet::new(),
        location,
    };
    if let Some(preflight) = &invocation.preflight {
        builder.stabilize(preflight, true, true)?;
    }
    builder.stabilize(
        &invocation.command,
        invocation.compiler,
        invocation.cargo && invocation.preflight.is_none(),
    )?;
    timing::report();
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
        self.targets.extend(timing::phase("proven", || {
            generated::proven(&self.workspaces)
        })?);
        for _ in 0..if replayable { 2 } else { 1 } {
            let declared = timing::phase("declared", || self.declared())?;
            let before = timing::phase("identities", || self.identities())?;
            self.targets.extend(
                declared
                    .iter()
                    .flat_map(|unit| unit.targets.iter().cloned()),
            );
            self.roots.extend(
                declared
                    .iter()
                    .flat_map(|unit| unit.directories.iter().cloned()),
            );
            self.declared
                .extend(declared.iter().flat_map(|unit| unit.paths.iter().cloned()));
            if invalidate {
                let invocation = Invocation::new(command);
                let outside: BTreeSet<&str> = declared
                    .iter()
                    .filter(|unit| !unit.trusted && !invocation.selects(&unit.package))
                    .map(|unit| unit.package.as_str())
                    .collect();
                if !outside.is_empty() {
                    return Err(outside_selection(
                        &outside,
                        failure::earlier_failure(&self.directories),
                    ));
                }
                declarations::invalidate(&declared)?;
            }
            let (entries, complete) = timing::phase("capture", || self.capture());
            timing::phase("run", || self.run(command, &entries, complete, &before))?;
            if !replayable
                || timing::phase("declared-after", || self.declared())?
                    .iter()
                    .all(|unit| unit.trusted)
            {
                return Ok(());
            }
        }
        Err(CiError::new(
            "build directory declarations did not stabilize after two compiler passes; a compiler pass must leave declared directory membership unchanged before inputs can be audited",
        ))
    }

    fn capture(&self) -> (Evidence, bool) {
        let mut evidence = Evidence {
            entries: BTreeMap::new(),
            declared: BTreeMap::new(),
            targets: self.targets.clone(),
        };
        for path in &self.declared {
            match inventory::state(path) {
                Ok(state) => {
                    evidence.declared.insert(path.clone(), state);
                }
                Err(_) => return (evidence, false),
            }
        }
        // The build scans its roots under the same default the audit uses.
        let mut budget = inventory::Budget::new(super::options::DEFAULT_MAX_ENTRIES);
        for root in &self.roots {
            if !root.exists() {
                continue;
            }
            match inventory::scan(root, &self.targets, &mut budget) {
                Ok(found) => evidence.entries.extend(found),
                Err(_) => return (evidence, false),
            }
        }
        (evidence, true)
    }

    fn identities(&self) -> Result<Identities, CiError> {
        let mut before = BTreeMap::new();
        for directory in &self.directories {
            for (unit, path) in unit_files(directory)? {
                if output(&unit) {
                    before.insert(path.clone(), identity(&path)?);
                }
            }
        }
        Ok(before)
    }

    fn target_directory(&self) -> Option<&Path> {
        self.workspaces
            .iter()
            .filter_map(|workspace| {
                let root = fs::canonicalize(&workspace.workspace_root).ok()?;
                self.location
                    .starts_with(&root)
                    .then_some((root.components().count(), workspace))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, workspace)| workspace.target_directory.as_path())
    }

    fn run(
        &self,
        command: &[String],
        entries: &Evidence,
        complete: bool,
        before: &Identities,
    ) -> Result<(), CiError> {
        let started = self
            .directories
            .iter()
            .map(|directory| clock::Clock::new(directory))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CiError::new(format!("cannot establish build filesystem clock: {error}"))
            })?;
        let mut build = Command::new(&command[0]);
        build.args(&command[1..]).current_dir(self.directory);
        if Invocation::new(command).cargo
            && std::env::var_os(TARGET_VARIABLE).is_none()
            && let Some(target) = self.target_directory()
        {
            build.env(TARGET_VARIABLE, target);
        }
        let status = build
            .status()
            .map_err(|e| CiError::new(format!("cannot run build command: {e}")))?;
        self.record(
            entries,
            complete,
            before,
            &started,
            status,
            Invocation::new(command).compiler,
        )?;
        if !status.success() {
            failure::record_failure(&self.directories, command)?;
            return Err(CiError::new(format!("build command failed: {status}")));
        }
        failure::clear_failure(&self.directories)
    }

    fn record(
        &self,
        entries: &Evidence,
        complete: bool,
        before: &Identities,
        started: &[clock::Clock],
        status: ExitStatus,
        compiler: bool,
    ) -> Result<(), CiError> {
        let mut evidence = entries.clone();
        if compiler && status.success() {
            evidence
                .targets
                .extend(generated::targets(&self.workspaces, entries, before)?);
        }
        let bytes = serde_json::to_vec(&evidence).map_err(|e| CiError::new(e.to_string()))?;
        let digest = sha256_hex(&bytes);
        for (directory, started) in self.directories.iter().zip(started) {
            let store = directory.join(inventory::STORE);
            let saved = store.join(format!("{digest}.json"));
            if complete && status.success() {
                fs::create_dir_all(&store)
                    .and_then(|()| fs::write(&saved, &bytes))
                    .map_err(|e| CiError::new(format!("cannot save input inventory: {e}")))?;
            }
            for (unit, file) in unit_files(directory)? {
                if !output(&unit) {
                    continue;
                }
                let current = identity(&file)?;
                let previous = before.get(&file);
                if previous == Some(&current) {
                    continue;
                }
                let receipt = inventory::receipt_path(&file);
                let newer = current.1 > started.modified
                    || previous.is_some() && current.1 == started.modified;
                if complete && status.success() && newer {
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

/// Packages named in the refusal before the list is cut off.
const LISTED_PACKAGES: usize = 20;

fn outside_selection(packages: &BTreeSet<&str>, earlier: Option<String>) -> CiError {
    let mut names: Vec<String> = packages
        .iter()
        .take(LISTED_PACKAGES)
        .map(|package| format!("{package} (build-script output)"))
        .collect();
    let more = packages.len().saturating_sub(LISTED_PACKAGES);
    if more > 0 {
        names.push(format!("and {more} more"));
    }
    let cause = match earlier {
        Some(command) => format!(
            "An earlier inputs build failed: `{command}` (failed commands record no receipts); fix that step first"
        ),
        None => "This usually means an earlier `velnor-ci inputs build` step in this job failed (failed commands record no receipts); fix that step first. Otherwise prepare inventory first with velnor-ci inputs build -- cargo build --workspace --all-targets, preserving this build's feature, target and profile options".to_owned(),
    };
    CiError::new(format!(
        "untrusted build outputs include packages outside this command's explicit selection: {}. {cause}",
        names.join(", ")
    ))
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
