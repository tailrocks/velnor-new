// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Component, Path};

use crate::cargo_artifacts::CargoArtifact;
use crate::report::LaneMetrics;
use crate::telemetry::Invocation;

pub(crate) fn identity(invocation: &Invocation, artifact: &CargoArtifact) -> Option<String> {
    let mut filenames: Vec<_> = artifact
        .filenames
        .iter()
        .filter(|filename| {
            let path = Path::new(filename);
            !path.is_absolute()
                && (path.parent().is_some_and(|parent| parent.ends_with("deps"))
                    && (matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("rmeta" | "rlib")
                    ) || artifact.profile.test
                        || (artifact.target.kind.iter().any(|kind| kind == "proc-macro")
                            && matches!(
                                path.extension().and_then(|value| value.to_str()),
                                Some("so" | "dylib" | "dll")
                            )))
                    && path
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.strip_prefix("lib")
                                .unwrap_or(name)
                                .strip_prefix(&artifact.target.name.replace('-', "_"))
                                .is_some_and(|suffix| suffix.starts_with('-') && suffix.len() > 1)
                        }))
        })
        .collect();
    if filenames.is_empty() {
        return None;
    }
    let mut features = artifact.features.clone();
    features.sort();
    features.dedup();
    filenames.sort();
    filenames.dedup();
    serde_json::to_string(&(
        &invocation.root,
        &artifact.package_id,
        &artifact.target,
        &artifact.profile,
        features,
        filenames,
        &invocation.execution,
    ))
    .ok()
}

pub fn collect(invocation: &Invocation, units: &mut BTreeMap<String, (String, u64)>) {
    for artifact in invocation
        .artifacts
        .iter()
        .filter(|artifact| !artifact.fresh)
    {
        if let Some(identity) = identity(invocation, artifact) {
            let entry = units
                .entry(identity)
                .or_insert_with(|| (artifact.target.name.clone(), 0));
            entry.1 += 1;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    Lint,
    Build,
}

/// `cargo clippy`, `check` and `doc` against `cargo build`, `test` and `nextest archive`: the two
/// jobs of the lint and test split run one each, in parallel, with a target cache apiece.
fn command_kind(invocation: &Invocation) -> CommandKind {
    let command = invocation
        .argv
        .iter()
        .skip(1)
        .find(|word| !word.starts_with('-') && !word.starts_with('+'));
    match command.map(String::as_str) {
        Some("check" | "c" | "clippy" | "doc" | "d") => CommandKind::Lint,
        _ => CommandKind::Build,
    }
}

/// Units that were compiled more than once where one compilation would have served.
///
/// A unit repeated inside one job is a miss. A unit repeated between jobs is a miss when both run
/// the same kind of command: they could have shared a cache or an archive. The lint job and the
/// test-archive job of the split are the exception, because they are parallel by design and
/// cannot share a target cache. What they both compile is the host side (proc-macro crates and the
/// crates only they need), which `clippy` and `nextest archive` build in the same way.
fn repeated(units: BTreeMap<String, (String, u64)>) -> Vec<String> {
    units
        .into_values()
        .filter(|(_, count)| *count > 1)
        .map(|(package, count)| {
            format!("{package}: identical observed compilation unit executed {count} times")
        })
        .collect()
}

/// Units that two cargo invocations of one lane both compiled.
#[must_use]
pub fn lane_repeats(lane: &LaneMetrics) -> Vec<String> {
    let mut within = BTreeMap::new();
    for invocation in &lane.invocations {
        collect(invocation, &mut within);
    }
    repeated(within)
}

#[must_use]
pub fn repeats(lanes: &[LaneMetrics]) -> Vec<String> {
    let mut groups: Vec<BTreeMap<String, (String, u64)>> = Vec::new();
    for lane in lanes {
        let mut within = BTreeMap::new();
        for invocation in &lane.invocations {
            collect(invocation, &mut within);
        }
        groups.push(within);
    }
    for kind in [CommandKind::Lint, CommandKind::Build] {
        let mut across = BTreeMap::new();
        for invocation in lanes
            .iter()
            .flat_map(|lane| &lane.invocations)
            .filter(|invocation| command_kind(invocation) == kind)
        {
            collect(invocation, &mut across);
        }
        groups.push(across);
    }
    groups.into_iter().flat_map(repeated).collect()
}

#[must_use]
pub fn diagnostics(invocation: &Invocation) -> Vec<String> {
    let unknown = invocation
        .artifacts
        .iter()
        .filter(|artifact| !artifact.fresh && identity(invocation, artifact).is_none())
        .count();
    if unknown == 0 && invocation.rejected == 0 {
        Vec::new()
    } else {
        vec![format!(
            "{}: compilation identity unknown for {unknown} artifacts; {} artifact records rejected",
            invocation.root, invocation.rejected
        )]
    }
}

pub fn normalize(invocation: &mut Invocation, checkout: &Path, target: &Path) {
    let relative = |text: &mut String| {
        let path = Path::new(text);
        if let Ok(relative) = path.strip_prefix(checkout) {
            *text = format!("checkout/{}", relative.display());
        }
    };
    for artifact in &mut invocation.artifacts {
        for filename in &mut artifact.filenames {
            if let Some(suffix) = target_relative(Path::new(filename), target) {
                *filename = format!("target/{}", suffix.display());
            } else {
                relative(filename);
            }
        }
        relative(&mut artifact.target.src_path);
        relative(&mut artifact.manifest_path);
        if (artifact.package_id.starts_with("registry+") || artifact.package_id.starts_with("git+"))
            && let Some(package) = Path::new(&artifact.manifest_path).parent()
            && let Ok(source) = Path::new(&artifact.target.src_path).strip_prefix(package)
        {
            artifact.target.src_path = format!("package/{}", source.display());
        }
        if let Some(package) = checkout_package(&artifact.package_id, checkout) {
            artifact.package_id = package;
        }
    }
    if let Ok(root) = Path::new(&invocation.root).strip_prefix(checkout) {
        invocation.root = if root.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            root.to_string_lossy().into_owned()
        };
    }
}

fn target_relative(path: &Path, target: &Path) -> Option<std::path::PathBuf> {
    let normalized_path: std::path::PathBuf = path
        .components()
        .fold(Vec::new(), |mut components, component| {
            push_component(&mut components, component);
            components
        })
        .into_iter()
        .collect();
    let normalized_target: std::path::PathBuf = target
        .components()
        .fold(Vec::new(), |mut components, component| {
            push_component(&mut components, component);
            components
        })
        .into_iter()
        .collect();
    normalized_path
        .strip_prefix(normalized_target)
        .ok()
        .map(Path::to_path_buf)
}

fn push_component<'a>(components: &mut Vec<Component<'a>>, component: Component<'a>) {
    match component {
        Component::CurDir => {}
        Component::ParentDir if matches!(components.last(), Some(Component::Normal(_))) => {
            components.pop();
        }
        Component::ParentDir if matches!(components.last(), Some(Component::RootDir)) => {}
        _ => components.push(component),
    }
}

fn checkout_package(package: &str, checkout: &Path) -> Option<String> {
    let (path, fragment) = package.strip_prefix("path+file://")?.split_once('#')?;
    if !path.starts_with('/') {
        return None;
    }
    let mut encoded = path.bytes();
    let mut decoded = Vec::with_capacity(path.len());
    while let Some(byte) = encoded.next() {
        decoded.push(if byte == b'%' {
            let high = char::from(encoded.next()?).to_digit(16)?;
            let low = char::from(encoded.next()?).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()?
        } else {
            byte
        });
    }
    let path = String::from_utf8(decoded).ok()?;
    let relative = Path::new(&path).strip_prefix(checkout).ok()?;
    let suffix = if relative.as_os_str().is_empty() {
        String::new()
    } else {
        format!("/{}", relative.display())
    };
    Some(format!("path+file://checkout{suffix}#{fragment}"))
}
