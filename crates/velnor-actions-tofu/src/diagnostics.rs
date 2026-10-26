// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::{ContractError, Finding};

use crate::effective::effective_set;
use crate::family::{Family, family_of};
use crate::file_cache::FileCache;
use crate::lockfile::{
    corrupt_lockfile_finding, inspect_lockfile, lock_digest_at_root, lock_relative,
};
use crate::parser::{FileModel, MAX_DIAGNOSTIC_CHARS};
use crate::task_identity::DigestSlot;
use crate::version::{admits_version, toolchain_triple};

pub const LOCKFILE_MISSING: &str = "tofu_lockfile_missing";
pub const LOCKFILE_STALE: &str = "tofu_lockfile_stale";
pub const REQUIRED_VERSION_EXCLUDES_TOOLCHAIN: &str = "tofu_required_version_excludes_toolchain";
pub const PROVIDER_DEPENDENCY_CHANGES: &str = "tofu_provider_dependency_changes";

const PROVIDER_CHANGES_MARKER: &str = "Provider dependency changes detected";
const MAX_CLAIMS_PER_ROOT: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredVersionClaim {
    pub path: String,
    pub constraint: String,
}

#[must_use]
pub fn lockfile_findings_for_root(
    root: &Path,
    unit_path: &str,
    reads: &mut FileCache,
) -> Vec<Finding> {
    if lock_digest_at_root(root, unit_path, &mut *reads).is_unknown() {
        return Vec::new();
    }
    let lock = lock_relative(unit_path);
    let Ok(bytes) = reads.read_raw(&root.join(&lock)) else {
        return match root_shape(root, unit_path, &mut *reads) {
            Some((true, _)) => vec![missing_finding(&lock)],
            Some((false, _)) | None => Vec::new(),
        };
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return vec![corrupt_lockfile_finding(&lock, "unreadable_utf8")];
    };
    let Ok(inspection) = inspect_lockfile(&lock, Some(&text)) else {
        return Vec::new();
    };
    if inspection.spec.is_none() {
        return inspection.findings;
    }
    let mut findings = inspection.findings;
    let providers = inspection.spec.map_or_else(Vec::new, |spec| spec.providers);
    match root_shape(root, unit_path, reads) {
        Some((needs, _)) if providers.is_empty() && needs => {
            findings.push(missing_finding(&lock));
        }
        Some((_, true)) if !providers.is_empty() => {
            findings.push(stale_finding(&lock, &providers));
        }
        Some(_) | None => {}
    }
    findings
}

pub fn require_committed_provider_lock(
    file: &str,
    root: &Path,
    unit_path: &str,
    reads: &mut FileCache,
) -> Result<(), ContractError> {
    let Some((needs, _)) = root_shape(root, unit_path, reads) else {
        return Ok(());
    };
    if !needs {
        return Ok(());
    }
    let lock = lock_relative(unit_path);
    match lock_digest_at_root(root, unit_path, reads) {
        DigestSlot::Known(_) => Ok(()),
        DigestSlot::AbsentProven(_) => Err(ContractError::config(
            file,
            "stacks.tofu.roots",
            format!(
                "missing_committed_lock:{lock}:commit a lockfile: run `tofu providers lock` \
                 manually and commit the result"
            ),
        )),
        DigestSlot::Unknown(_) => Err(ContractError::config(
            file,
            "stacks.tofu.roots",
            format!("unreadable_committed_lock:{lock}"),
        )),
    }
}

#[must_use]
pub fn required_versions_for_root(
    root: &Path,
    unit_path: &str,
    reads: &mut FileCache,
) -> Vec<RequiredVersionClaim> {
    let mut claims = Vec::new();
    let Some(paths) = effective_paths(root, unit_path, &mut *reads) else {
        return claims;
    };
    for path in &paths {
        let Some(model) = parse_model(root, path, &mut *reads) else {
            continue;
        };
        for constraint in &model.required_versions {
            if claims.len() >= MAX_CLAIMS_PER_ROOT {
                return claims;
            }
            claims.push(RequiredVersionClaim {
                path: path.clone(),
                constraint: constraint.clone(),
            });
        }
    }
    claims
}

#[must_use]
pub fn version_compat_findings(claims: &[RequiredVersionClaim], toolchain: &str) -> Vec<Finding> {
    let Some(triple) = toolchain_triple(toolchain) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for claim in claims {
        if !admits_version(&claim.constraint, triple) {
            findings.push(excludes_finding(claim, toolchain));
        }
    }
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.observed.cmp(&right.observed))
    });
    findings
}

#[must_use]
pub fn remediation_for_init_stderr(
    lock_path: &str,
    stderr: &str,
    exit_code: i32,
) -> Option<Finding> {
    if exit_code != 1 || !stderr.contains(PROVIDER_CHANGES_MARKER) {
        return None;
    }
    Some(Finding {
        code: PROVIDER_DEPENDENCY_CHANGES.to_owned(),
        path: lock_path.to_owned(),
        observed: Some(format!("readonly init exit 1: {PROVIDER_CHANGES_MARKER}")),
        recommended: None,
        action: Some(
            "run `tofu providers lock` for the changed selection manually and commit the \
             updated lock; never delete the lockfile"
                .to_owned(),
        ),
        reason: "the committed lock no longer matches the required providers; validate cannot \
             proceed until a human refreshes it"
            .to_owned(),
    })
}

fn root_shape(root: &Path, unit_path: &str, reads: &mut FileCache) -> Option<(bool, bool)> {
    let paths = effective_paths(root, unit_path, &mut *reads)?;
    let mut needs = false;
    let mut bare = true;
    for path in &paths {
        let model = parse_model(root, path, &mut *reads)?;
        for block in &model.blocks {
            match block.kind.as_str() {
                "resource" | "data" | "provider" => {
                    needs = true;
                    bare = false;
                }
                "terraform" | "module" => {
                    bare = false;
                }
                _ => {}
            }
        }
    }
    Some((needs, bare))
}

fn effective_paths(root: &Path, unit_path: &str, reads: &mut FileCache) -> Option<Vec<String>> {
    let unit = if unit_path == "." { "" } else { unit_path };
    let collected = reads.unit_files(root, unit).ok()?;
    let configs: Vec<String> = collected
        .into_iter()
        .filter(|path| {
            matches!(
                family_of(path.rsplit('/').next().unwrap_or(path)),
                Family::Config | Family::Override
            )
        })
        .collect();
    Some(effective_set(&configs))
}

fn parse_model(root: &Path, path: &str, reads: &mut FileCache) -> Option<FileModel> {
    reads.model_for(root, path).ok().flatten()
}

fn missing_finding(lock: &str) -> Finding {
    Finding {
        code: LOCKFILE_MISSING.to_owned(),
        path: lock.to_owned(),
        observed: None,
        recommended: None,
        action: Some(
            "commit a lockfile for this provider root: run `tofu providers lock` manually and \
             commit the result; Velnor never creates it"
                .to_owned(),
        ),
        reason: "provider roots validate against the committed lock; without pins validate \
             cannot prove the provider set"
            .to_owned(),
    }
}

fn stale_finding(lock: &str, providers: &[String]) -> Finding {
    let observed: String = providers
        .join(",")
        .chars()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect();
    Finding {
        code: LOCKFILE_STALE.to_owned(),
        path: lock.to_owned(),
        observed: Some(observed),
        recommended: None,
        action: Some(
            "refresh the lockfile manually (`tofu providers lock`) or remove the stale entries \
             and commit; Velnor never repairs or deletes the lock"
                .to_owned(),
        ),
        reason: "stale entries fail validate with `no package cached`; the root declares no \
             providers"
            .to_owned(),
    }
}

fn excludes_finding(claim: &RequiredVersionClaim, toolchain: &str) -> Finding {
    Finding {
        code: REQUIRED_VERSION_EXCLUDES_TOOLCHAIN.to_owned(),
        path: claim.path.clone(),
        observed: Some(format!(
            "{} excludes toolchain {toolchain}",
            claim.constraint
        )),
        recommended: None,
        action: Some(
            "align the required_version constraint with the OpenTofu toolchain pin manually; \
             Velnor keeps its own exact pin"
                .to_owned(),
        ),
        reason: "generated steps run the pinned toolchain, but the root forbids it".to_owned(),
    }
}
