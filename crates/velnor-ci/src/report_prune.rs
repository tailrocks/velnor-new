// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::telemetry::Invocation;

pub(crate) fn outputs_only(invocation: &Invocation, package: &str, version: &str) -> bool {
    let mut saw = false;
    for artifact in &invocation.artifacts {
        if artifact.fresh {
            continue;
        }
        let Some(identity) = crate::cargo_package::PackageIdentity::parse(&artifact.package_id)
        else {
            continue;
        };
        if identity.name != package
            || identity.version != version
            || identity
                .source
                .is_none_or(|source| !source.starts_with("path+"))
        {
            continue;
        }
        saw = true;
        if retained_build_script(artifact)
            || artifact.filenames.is_empty()
            || artifact
                .filenames
                .iter()
                .any(|file| !pruned_output(file, invocation.target_directory.as_deref()))
        {
            return false;
        }
    }
    saw
}

fn retained_build_script(artifact: &crate::cargo_artifacts::CargoArtifact) -> bool {
    artifact
        .target
        .kind
        .iter()
        .any(|kind| kind == "custom-build")
        || artifact.filenames.iter().any(|file| {
            let name = file_name(file);
            name.starts_with("build_script_") || name.starts_with("build-script-")
        })
}

fn pruned_output(file: &str, target_directory: Option<&str>) -> bool {
    let normalized = file.replace('\\', "/");
    let name = file_name(&normalized);
    if name.starts_with("build_script_") || name.starts_with("build-script-") {
        return false;
    }
    crate::cache_prune::is_executable(name) || doc_output(&normalized, target_directory)
}

fn doc_output(file: &str, target_directory: Option<&str>) -> bool {
    if let Some(dir) = target_directory {
        let dir = dir.replace('\\', "/");
        let prefix = dir.trim_end_matches('/');
        let Some(rest) = file.strip_prefix(prefix) else {
            return false;
        };
        let rest = rest.trim_start_matches('/');
        return rest == "doc" || rest.starts_with("doc/");
    }
    let parts: Vec<&str> = file.split('/').collect();
    parts
        .windows(2)
        .any(|pair| pair[0] == "target" && pair[1] == "doc")
        || file.starts_with("doc/")
}

fn file_name(file: &str) -> &str {
    file.rsplit(['/', '\\']).next().unwrap_or(file)
}
