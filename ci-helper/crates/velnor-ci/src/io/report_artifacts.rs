// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cargo_artifacts::{CargoArtifact, CargoProgress, ProgressKind};
use crate::cargo_package::PackageIdentity;
use crate::telemetry::{Compilation, Invocation};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

struct Package {
    key: String,
    name: String,
    version: String,
}

pub(super) fn compilations(
    invocation: &Invocation,
    root: &velnor_model::RustRoot,
    checkout: &Path,
    unchanged: &BTreeSet<String>,
    progress: &[CargoProgress],
) -> (Vec<Compilation>, Vec<String>) {
    let mut artifacts = BTreeMap::new();
    let mut errors = Vec::new();
    for artifact in invocation
        .artifacts
        .iter()
        .filter(|artifact| !artifact.fresh)
    {
        let Some(package) = package(&artifact.package_id) else {
            errors.push(format!(
                "{}: non-fresh artifact package identity is unsupported",
                invocation.root
            ));
            continue;
        };
        artifacts
            .entry(package.key.clone())
            .or_insert((package, artifact));
    }
    let mut represented = BTreeSet::new();
    let mut compilations = Vec::new();
    for event in progress.iter().filter(|event| is_compilation(event.kind)) {
        let matching: Vec<_> = artifacts
            .iter()
            .filter(|(_, (package, artifact))| {
                package.name == event.package
                    && package.version == event.version
                    && event
                        .location
                        .as_deref()
                        .is_none_or(|location| artifact_location(artifact, location))
            })
            .map(|(key, _)| key.clone())
            .collect();
        if matching.len() > 1 && event.location.is_none() {
            continue;
        }
        if let [key] = matching.as_slice() {
            represented.insert(key.clone());
        }
        compilations.push(from_progress(root, checkout, unchanged, event));
    }
    for (key, (package, artifact)) in artifacts {
        if represented.contains(&key) {
            continue;
        }
        compilations.push(from_artifact(root, checkout, unchanged, &package, artifact));
    }
    (compilations, errors)
}

pub(super) fn downloads(
    invocation: &Invocation,
    root: &str,
    progress: &[CargoProgress],
    exact_sources: bool,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let events: Vec<_> = progress
        .iter()
        .filter(|event| event.kind == ProgressKind::Downloaded)
        .collect();
    match &invocation.verified_download_archives {
        Some(archives) => verified_downloads(archives, root, &events),
        None if exact_sources => (
            Vec::new(),
            Vec::new(),
            vec![format!(
                "{root}: crate archive evidence is unavailable for an exact sources cache hit"
            )],
        ),
        None => (
            events
                .into_iter()
                .map(|event| format!("{} v{}", event.package, event.version))
                .collect(),
            Vec::new(),
            Vec::new(),
        ),
    }
}

fn verified_downloads(
    archives: &[String],
    root: &str,
    events: &[&CargoProgress],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let archives: BTreeSet<_> = archives.iter().collect();
    let mut downloads = Vec::new();
    for archive in &archives {
        let label = if let Some(event) = events.iter().find(|event| {
            Path::new(archive).file_name().is_some_and(|name| {
                name == format!("{}-{}.crate", event.package, event.version).as_str()
            })
        }) {
            format!("{} v{} ({archive})", event.package, event.version)
        } else {
            (*archive).clone()
        };
        downloads.push(label);
    }
    let diagnostics = events
        .iter()
        .filter(|event| {
            let basename = format!("{}-{}.crate", event.package, event.version);
            !archives.iter().any(|archive| {
                Path::new(archive)
                    .file_name()
                    .is_some_and(|name| name == basename.as_str())
            })
        })
        .map(|event| {
            format!(
                "{root}: ignored unverified download event for {} v{}",
                event.package, event.version
            )
        })
        .collect();
    (downloads, diagnostics, Vec::new())
}

fn package(package_id: &str) -> Option<Package> {
    let identity = PackageIdentity::parse(package_id)?;
    let source = identity.source?;
    Some(Package {
        key: format!("{source}#{}@{}", identity.name, identity.version),
        name: identity.name.to_owned(),
        version: identity.version.to_owned(),
    })
}

pub(super) fn is_compilation(kind: ProgressKind) -> bool {
    matches!(
        kind,
        ProgressKind::Compiling | ProgressKind::Checking | ProgressKind::Documenting
    )
}

fn artifact_location(artifact: &CargoArtifact, location: &str) -> bool {
    Path::new(&artifact.manifest_path).parent() == Some(Path::new(location))
        || git_location(&artifact.package_id, location)
}

fn git_location(package_id: &str, location: &str) -> bool {
    let Some((source, _)) = package_id.rsplit_once('#') else {
        return false;
    };
    let Some(source) = source.strip_prefix("git+") else {
        return false;
    };
    let Some((repository, revision)) = location.rsplit_once('#') else {
        return false;
    };
    if source != repository {
        return false;
    }
    let expected = source
        .split_once('?')
        .and_then(|(_, query)| query.split('&').find_map(|part| part.strip_prefix("rev=")))
        .filter(|value| value.len() >= 7 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    expected.is_none_or(|expected| !revision.is_empty() && expected.starts_with(revision))
}

fn from_progress(
    root: &velnor_model::RustRoot,
    checkout: &Path,
    unchanged: &BTreeSet<String>,
    event: &CargoProgress,
) -> Compilation {
    let member = super::member(root, checkout, event);
    Compilation {
        unchanged: unchanged.contains(event.package.as_str()),
        workspace: member.is_some(),
        dependencies: super::dependencies(root, member),
        package: event.package.clone(),
        version: event.version.clone(),
    }
}

fn from_artifact(
    root: &velnor_model::RustRoot,
    checkout: &Path,
    unchanged: &BTreeSet<String>,
    package: &Package,
    artifact: &CargoArtifact,
) -> Compilation {
    let location = PathBuf::from(&artifact.manifest_path)
        .parent()
        .map(Path::to_path_buf)
        .map(|path| path.to_string_lossy().into_owned());
    let event = CargoProgress {
        kind: ProgressKind::Compiling,
        package: package.name.clone(),
        version: package.version.clone(),
        location,
    };
    from_progress(root, checkout, unchanged, &event)
}
