// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_spec::validate_source_sha;

use crate::OrchestratorError;
use crate::planner::origin::origin_url_via_git;

pub(crate) fn origin_repository(root: &Path) -> Result<String, OrchestratorError> {
    let Some(url) = origin_url_via_git(root) else {
        return Err(OrchestratorError::IdentityRejected {
            problem: "release_origin_unresolvable:set remote origin to the repository URL"
                .to_owned(),
        });
    };
    normalize_origin(&url).ok_or_else(|| OrchestratorError::IdentityRejected {
        problem: "release_origin_not_github:origin must be a github.com owner/repo URL".to_owned(),
    })
}

fn normalize_origin(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some((host, path)) = crate::planner::prepare::split_host_path(base) {
        if host.eq_ignore_ascii_case("github.com") && is_owner_repo(path) {
            return Some(path.to_owned());
        }
        return None;
    }
    is_owner_repo(base).then(|| base.to_owned())
}

fn is_owner_repo(path: &str) -> bool {
    let Some((owner, name)) = path.split_once('/') else {
        return false;
    };
    !name.contains('/') && is_repo_segment(owner) && is_repo_segment(name)
}

pub(crate) fn same_repository(left: &str, right: &str) -> bool {
    let valid = |value: &str| {
        value.split_once('/').is_some_and(|(owner, name)| {
            velnor_actions_contract::config::is_valid_github_owner(owner)
                && velnor_actions_contract::config::is_valid_github_repo_name(name)
        })
    };
    valid(left) && valid(right) && left.eq_ignore_ascii_case(right)
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(crate) fn head_sha(root: &Path) -> Result<String, OrchestratorError> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in(root)
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    if !output.success {
        return Err(OrchestratorError::Contract {
            problem: "release_head_unresolvable:commit before generating release workflows"
                .to_owned(),
        });
    }
    let sha = output
        .stdout_text("git")
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let sha = sha.trim().to_owned();
    validate_source_sha(&sha)?;
    Ok(sha)
}

pub(crate) fn plan_id_for_source(sha: &str) -> String {
    format!("release-{}", &sha[..12])
}

pub(crate) fn common_registry(selection: &ReleaseSelection) -> Result<String, OrchestratorError> {
    let mut common: Option<BTreeSet<&str>> = None;
    for package in &selection.packages {
        let registries: BTreeSet<&str> = package.registries.iter().map(String::as_str).collect();
        common = Some(match common {
            None => registries,
            Some(kept) => kept.intersection(&registries).copied().collect(),
        });
    }
    common
        .and_then(|kept| kept.into_iter().next().map(str::to_owned))
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "release_no_common_registry".to_owned(),
        })
}

pub(crate) fn workspace_lock(
    metadata: &str,
    root: &Path,
    manifest_hint: &str,
    known_manifests: &BTreeSet<String>,
    registry: &str,
    repository: &str,
) -> Result<
    velnor_actions_workflow_renderer::release_spec::lock::ReleaseConcurrency,
    OrchestratorError,
> {
    use sha2::{Digest, Sha256};
    let record = velnor_actions_rust::metadata::parse_metadata_json(
        metadata,
        root,
        manifest_hint,
        known_manifests,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let root = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
    let workspace = root
        .join(&record.workspace_root)
        .canonicalize()
        .map_err(|err| OrchestratorError::io(record.workspace_root.clone(), err.to_string()))?;
    let relative = workspace
        .strip_prefix(&root)
        .map_err(|_| OrchestratorError::Contract {
            problem: "release_workspace_outside_root".to_owned(),
        })?;
    let manifest = root
        .join(manifest_hint)
        .canonicalize()
        .map_err(|err| OrchestratorError::io(manifest_hint, err.to_string()))?;
    let reported_member = record
        .packages
        .iter()
        .filter(|package| package.in_workspace)
        .any(|package| {
            root.join(&package.manifest)
                .canonicalize()
                .is_ok_and(|path| path == manifest)
        });
    let members_within_workspace = record
        .packages
        .iter()
        .filter(|package| package.in_workspace)
        .all(|package| {
            root.join(&package.manifest)
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&workspace))
        });
    if !workspace.is_dir()
        || !manifest.is_file()
        || !manifest.starts_with(&workspace)
        || !members_within_workspace
        || (manifest != workspace.join("Cargo.toml") && !reported_member)
    {
        return Err(OrchestratorError::Contract {
            problem: "release_workspace_manifest_mismatch".to_owned(),
        });
    }
    let components = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "release_workspace_invalid_path".to_owned(),
        })?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(components.join("/").as_bytes()) {
        use std::fmt::Write as _;
        write!(&mut digest, "{byte:02x}").map_err(|_| OrchestratorError::Contract {
            problem: "release_workspace_digest_encoding".to_owned(),
        })?;
    }
    velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group(
        registry, repository, &digest,
    )
    .map_err(Into::into)
}

#[cfg(test)]
#[path = "release_identity_tests.rs"]
mod tests;
