// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use crate::cargo_artifacts::CargoArtifact;
use crate::report::Invocation;

fn identity(invocation: &Invocation, artifact: &CargoArtifact) -> Option<String> {
    if artifact.filenames.is_empty()
        || !artifact.filenames.iter().all(|filename| {
            let path = Path::new(filename);
            path.parent().is_some_and(|parent| parent.ends_with("deps"))
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
                    })
        })
    {
        return None;
    }
    let mut features = artifact.features.clone();
    features.sort();
    features.dedup();
    let mut filenames = artifact.filenames.clone();
    filenames.sort();
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

pub(crate) fn collect(invocation: &Invocation, units: &mut BTreeMap<String, (String, u64)>) {
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

pub(crate) fn diagnostics(invocation: &Invocation) -> Vec<String> {
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

pub(crate) fn normalize(invocation: &mut Invocation, checkout: &Path) {
    let cwd = std::path::PathBuf::from(&invocation.root);
    let target = invocation
        .target_directory
        .as_ref()
        .map_or_else(|| cwd.join("target"), std::path::PathBuf::from);
    let relative = |text: &mut String| {
        let path = Path::new(text);
        if let Ok(relative) = path.strip_prefix(&target) {
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
        let prefix = format!("path+file://{}", checkout.display());
        if let Some(suffix) = artifact.package_id.strip_prefix(&prefix)
            && (suffix.starts_with('/') || suffix.starts_with('#'))
        {
            artifact.package_id = format!("path+file://checkout{suffix}");
        }
    }
}
