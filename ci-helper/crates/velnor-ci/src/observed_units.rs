// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use crate::cargo_artifacts::CargoArtifact;
use crate::telemetry::Invocation;

pub(crate) fn identity(invocation: &Invocation, artifact: &CargoArtifact) -> Option<String> {
    let mut filenames: Vec<_> = artifact
        .filenames
        .iter()
        .filter(|filename| {
            let path = Path::new(filename);
            !path.is_absolute()
                && (path.ends_with(
                    Path::new("doc")
                        .join(artifact.target.name.replace('-', "_"))
                        .join("index.html"),
                ) || (path.parent().is_some_and(|parent| parent.ends_with("deps"))
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
                        })))
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
            "{}: compilation identity unknown for {unknown} artifacts; {} malformed artifact records rejected",
            invocation.root, invocation.rejected
        )]
    }
}

pub fn normalize(invocation: &mut Invocation, checkout: &Path) {
    let target = invocation
        .target_directory
        .as_ref()
        .map(std::path::PathBuf::from);
    let relative = |text: &mut String| {
        let path = Path::new(text);
        if let Some(target) = &target
            && let Ok(relative) = path.strip_prefix(target)
        {
            *text = format!("target/{}", relative.display());
        } else if let Ok(relative) = path.strip_prefix(checkout) {
            *text = format!("checkout/{}", relative.display());
        }
    };
    for artifact in &mut invocation.artifacts {
        for filename in &mut artifact.filenames {
            relative(filename);
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
