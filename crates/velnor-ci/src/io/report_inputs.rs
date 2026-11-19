// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cargo_artifacts::{ArtifactTap, ProgressKind};
use crate::error::CiError;
use crate::inputs::{SYMLINK_MODE, snapshot_file, witness_file};
use crate::report::{Hit, LaneMetrics};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use velnor_model::{AllTrigger, ChangeSet, RepoPath, RustSelection};

#[path = "report_artifacts.rs"]
mod artifact_evidence;

fn ci(error: impl std::fmt::Display) -> CiError {
    CiError::new(error.to_string())
}

pub(super) fn collect_inputs(metrics: &mut LaneMetrics, log: &[u8]) -> Result<(), CiError> {
    let directory = super::current_dir()?;
    let report = super::report_checkout::read(&directory)?;
    let facts = velnor_rust::detect(&report.checkout.view).map_err(ci)?;
    for root in &facts.rust_roots {
        collect_root(metrics, &facts, root, &report, log)?;
    }
    Ok(())
}

fn collect_root(
    metrics: &mut LaneMetrics,
    facts: &velnor_model::RepoFacts,
    root: &velnor_model::RustRoot,
    report: &super::report_checkout::ReportCheckout,
    log: &[u8],
) -> Result<(), CiError> {
    let checkout = &report.checkout.root;
    let directory = root.manifest.parent();
    let root_text = directory.as_ref().map_or(".", RepoPath::as_str);
    let root_path = checkout.join(directory.as_ref().map_or("", RepoPath::as_str));
    let prefix = super::input_scopes::git_prefix(&root_path)?;
    let scope = crate::scope::Scope::new(&prefix, &facts.compile_inputs, Some(facts));
    let (_, current) = super::input_scopes::scoped_files(&root_path, Some(&scope))?;
    let lock = root.manifest.sibling(&RepoPath::from_static("Cargo.lock"));
    let symlink_lock = report.symlink_locks.contains(&lock);
    let target = checkout.join(root.target_dir.as_str());
    let saved = read_snapshot(&target.join(snapshot_file(&prefix)))?;
    let saved_witness = read_witness(&target.join(witness_file(&prefix)))?;
    let usable = saved.is_some();
    let unverifiable_links = !current.links.is_empty()
        || current.unverifiable_links > 0
        || saved
            .as_ref()
            .is_some_and(|saved| saved.iter().any(|input| input.file.mode == SYMLINK_MODE));
    let same_lock = !symlink_lock && same_lock(&root.manifest, &current, saved.as_deref())?;
    let unchanged = if symlink_lock || unverifiable_links {
        BTreeSet::new()
    } else {
        unchanged(facts, root, &current, saved)
    };
    for cache in &mut metrics.caches {
        if cache.family == "target" && cache.root.as_deref() == Some(root_text) {
            cache.usable = usable && cache.hit != Hit::Miss;
            cache.usable_same_lock = cache.usable && same_lock;
        }
    }
    collect_invocations(
        metrics,
        &RootInputs {
            root,
            checkout,
            current: &current,
            symlink_lock,
            unverifiable_links,
            log,
            root_text,
            root_path: &root_path,
            unchanged: &unchanged,
            saved_witness: saved_witness.as_deref(),
        },
    )
}

struct RootInputs<'a> {
    root: &'a velnor_model::RustRoot,
    checkout: &'a Path,
    current: &'a crate::inputs::Tracked,
    log: &'a [u8],
    root_text: &'a str,
    root_path: &'a Path,
    unchanged: &'a BTreeSet<String>,
    saved_witness: Option<&'a str>,
    symlink_lock: bool,
    unverifiable_links: bool,
}

fn read_snapshot(path: &Path) -> Result<Option<Vec<crate::inputs::SavedInput>>, CiError> {
    match fs::read_to_string(path) {
        Ok(text) => super::input_snapshot::decode(&text).map(|snapshot| Some(snapshot.inputs)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ci(format!("cannot read {}: {error}", path.display()))),
    }
}

fn read_witness(path: &Path) -> Result<Option<String>, CiError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ci(format!("cannot read {}: {error}", path.display()))),
    }
}

fn collect_invocations(metrics: &mut LaneMetrics, inputs: &RootInputs<'_>) -> Result<(), CiError> {
    let warm = metrics.caches.iter().any(|cache| {
        cache.family == "target" && cache.root.as_deref() == Some(inputs.root_text) && cache.usable
    });
    for invocation in &mut metrics.invocations {
        if !same_directory(Path::new(&invocation.root), inputs.root_path) {
            continue;
        }
        let progress = progress(invocation, inputs.log)?;
        let progress = crate::progress_evidence::retain(
            invocation,
            progress,
            &mut metrics.diagnostics,
            &mut metrics.verification_errors,
        );
        let (compilations, errors) = artifact_evidence::compilations(
            invocation,
            inputs.root,
            inputs.checkout,
            inputs.unchanged,
            &progress,
        );
        metrics.verification_errors.extend(errors);
        invocation.compilations = compilations;
        let target = inputs.checkout.join(inputs.root.target_dir.as_str());
        let warm = warm
            && invocation
                .target_directory
                .as_deref()
                .is_none_or(|actual| same_directory(Path::new(actual), &target));
        invocation.restored_target_usable = Some(warm);
        if warm && inputs.symlink_lock && !invocation.compilations.is_empty() {
            metrics.verification_errors.push(format!(
                "{}: a tracked Cargo.lock symlink prevents compilation input verification",
                inputs.root_text
            ));
        }
        invocation.downloads = progress
            .iter()
            .filter(|event| event.kind == ProgressKind::Downloaded)
            .map(|event| format!("{} v{}", event.package, event.version))
            .collect();
        let target = invocation
            .target_directory
            .as_ref()
            .map_or(target, std::path::PathBuf::from);
        crate::observed_units::normalize(invocation, inputs.checkout, &target);
        inputs.root_text.clone_into(&mut invocation.root);
        verify_witness(
            &mut metrics.verification_errors,
            invocation,
            inputs.root_text,
            inputs.current,
            inputs.unverifiable_links && !inputs.symlink_lock,
            inputs.saved_witness,
            warm,
        );
    }
    Ok(())
}

fn same_directory(actual: &Path, expected: &Path) -> bool {
    match (fs::canonicalize(actual), fs::canonicalize(expected)) {
        (Ok(actual), Ok(expected)) => actual == expected,
        _ => actual == expected,
    }
}

pub(super) fn verify_downloads(metrics: &mut LaneMetrics, log: &[u8]) -> Result<(), CiError> {
    for invocation in &mut metrics.invocations {
        let progress = progress(invocation, log)?;
        let exact_sources = metrics
            .caches
            .iter()
            .any(|cache| cache.exact_sources_for(&invocation.root));
        let (downloads, diagnostics, errors) =
            artifact_evidence::downloads(invocation, &invocation.root, &progress, exact_sources);
        invocation.downloads = downloads;
        metrics.diagnostics.extend(diagnostics);
        metrics.verification_errors.extend(errors);
    }
    Ok(())
}

fn verify_witness(
    errors: &mut Vec<String>,
    invocation: &crate::telemetry::Invocation,
    root: &str,
    current: &crate::inputs::Tracked,
    unverifiable_links: bool,
    saved: Option<&str>,
    warm: bool,
) {
    if !warm || invocation.compilations.is_empty() {
        return;
    }
    if saved.is_none_or(|saved| saved != crate::io::inputs::input_witness(current)) {
        errors.push(format!(
            "{root}: restored input witness is missing or changed during Cargo"
        ));
    } else if unverifiable_links {
        errors.push(format!(
            "{root}: tracked symlink referents cannot verify compilation inputs"
        ));
    }
}

fn same_lock(
    manifest: &RepoPath,
    current: &crate::inputs::Tracked,
    saved: Option<&[crate::inputs::SavedInput]>,
) -> Result<bool, CiError> {
    let lock = manifest.sibling(&RepoPath::new("Cargo.lock").map_err(ci)?);
    Ok(saved.is_some_and(|saved| {
        saved
            .iter()
            .find(|input| input.file.path == lock.as_str())
            .is_some_and(|input| current.files.contains(&input.file))
    }))
}

fn unchanged(
    facts: &velnor_model::RepoFacts,
    root: &velnor_model::RustRoot,
    current: &crate::inputs::Tracked,
    saved: Option<Vec<crate::inputs::SavedInput>>,
) -> BTreeSet<String> {
    let mut unchanged = BTreeSet::new();
    if current.unsupported > 0 || current.unverifiable_links > 0 || !current.absent.is_empty() {
        return unchanged;
    }
    let current: BTreeMap<_, _> = current
        .files
        .iter()
        .chain(&current.links)
        .map(|file| (&file.path, file))
        .collect();
    if let Some(saved) = saved {
        let saved: BTreeMap<_, _> = saved
            .iter()
            .map(|input| (&input.file.path, &input.file))
            .collect();
        let prefix = root
            .manifest
            .parent()
            .map_or_else(String::new, |directory| format!("{directory}/"));
        let scope = crate::scope::Scope::new(&prefix, &facts.compile_inputs, Some(facts));
        let current: BTreeMap<_, _> = current
            .iter()
            .filter(|(path, _)| {
                scope.contains_bytes(path.as_bytes())
                    || RepoPath::new(path.as_str()).is_ok_and(|path| {
                        facts
                            .config
                            .inputs
                            .iter()
                            .any(|rule| rule.glob.matches(&path))
                    })
            })
            .map(|(path, file)| (*path, *file))
            .collect();
        let changes = ChangeSet {
            paths: saved
                .keys()
                .chain(current.keys())
                .filter(|path| saved.get(*path) != current.get(*path))
                .copied()
                .filter_map(|path| RepoPath::new(path.as_str()).ok())
                .collect(),
            ..ChangeSet::default()
        };
        let selection = velnor_rust::select(facts, &changes);
        for member in &root.members {
            if matches!(selection.get(&root.manifest), Some(RustSelection::None))
                || matches!(selection.get(&root.manifest), Some(RustSelection::Some(packages) | RustSelection::All(AllTrigger::MostPackages(packages))) if !packages.contains_key(&member.name))
            {
                unchanged.insert(member.name.to_string());
            }
        }
    }

    unchanged
}

fn dependencies(
    root: &velnor_model::RustRoot,
    member: Option<&velnor_model::RustMember>,
) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let origin = member.map(|member| &member.name);
    let mut pending: Vec<_> = member
        .into_iter()
        .flat_map(|member| &member.dependencies)
        .map(|dependency| &dependency.name)
        .collect();
    while let Some(name) = pending.pop() {
        if origin.is_none_or(|origin| name != origin)
            && dependencies.insert(name.to_string())
            && let Some(member) = root.members.iter().find(|member| member.name == *name)
        {
            pending.extend(
                member
                    .dependencies
                    .iter()
                    .map(|dependency| &dependency.name),
            );
        }
    }
    dependencies
}

fn progress(
    invocation: &crate::telemetry::Invocation,
    log: &[u8],
) -> Result<Vec<crate::cargo_artifacts::CargoProgress>, CiError> {
    if let Some(progress) = &invocation.progress {
        return Ok(progress.clone());
    }
    let slice = log
        .get(invocation.log_start..invocation.log_end)
        .ok_or_else(|| CiError::new("invocation log range is invalid"))?;
    let mut tap = ArtifactTap::default();
    tap.feed(slice);
    Ok(tap.finish().progress)
}

fn member<'a>(
    root: &'a velnor_model::RustRoot,
    checkout: &Path,
    event: &crate::cargo_artifacts::CargoProgress,
) -> Option<&'a velnor_model::RustMember> {
    root.members.iter().find(|member| {
        member.name.as_str() == event.package
            && event.location.as_deref()
                == Some(
                    member
                        .directory
                        .as_ref()
                        .map_or_else(
                            || checkout.to_path_buf(),
                            |directory| checkout.join(directory.as_str()),
                        )
                        .to_string_lossy()
                        .as_ref(),
                )
    })
}
