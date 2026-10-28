// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_actions_contract::ids::is_lower_hex_len;
use velnor_actions_contract::{GeneratorLock, ReleaseManifest, ReleaseTarget, TargetRecord};

use super::{
    PinnedTool, ToolCatalog,
    lock::{LockError, mismatch},
};

const PUBLISHED_VERDICT_VERSION: &str = "0.1.66";
const PUBLISHED_VERDICT_GH_VERSION: &str = "2.103.3";
const PUBLISHED_VERDICT_COMMIT: &str = "ecee0521d8631315790662107231dabf2ba6b1ec";
const PUBLISHED_VERDICT_LINUX_X64_SHA256: &str =
    "fe7cc33aae28b9be2fe9a0561e148c666516f30e53d65a069a6f3f5575a53298";
const PUBLISHED_VERDICT_MACOS_ARM64_SHA256: &str =
    "a319646c4c92bfdfc9abd623883338c2d18ef7e5549e00633dcba178fdd45e9d";
const PUBLISHED_VERDICT_MACOS_X64_SHA256: &str =
    "8193c279607c971da1d599e53b8ba6a1c87ba01f4ddc9c2fe993f8512ef852cf";

#[must_use]
pub fn published_verdict_manifest() -> ReleaseManifest {
    ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: PUBLISHED_VERDICT_VERSION.to_owned(),
        repository: velnor_actions_contract::targets::EXPECTED_REPOSITORY.to_owned(),
        commit: PUBLISHED_VERDICT_COMMIT.to_owned(),
        targets: vec![
            published_verdict_target(
                ReleaseTarget::LinuxX86_64,
                PUBLISHED_VERDICT_LINUX_X64_SHA256,
            ),
            published_verdict_target(
                ReleaseTarget::MacosArm64,
                PUBLISHED_VERDICT_MACOS_ARM64_SHA256,
            ),
            published_verdict_target(
                ReleaseTarget::MacosX86_64,
                PUBLISHED_VERDICT_MACOS_X64_SHA256,
            ),
        ],
    }
}

#[must_use]
pub fn catalog_for_published_verdict(catalog: &ToolCatalog) -> ToolCatalog {
    let mut verdict = catalog.clone();
    PUBLISHED_VERDICT_GH_VERSION.clone_into(&mut verdict.gh);
    verdict.project.remove(&PinnedTool::Gh);
    verdict
}

#[derive(Debug, Clone, Copy)]
pub struct ApprovedGeneratorRelease<'a> {
    pub manifest: &'a ReleaseManifest,
    pub gh_version: &'a str,
    pub mise_bootstrap_version: &'a str,
    pub mise_bootstrap_artifact: &'a str,
    pub mise_bootstrap_sha256: &'a str,
    pub mise_runtime: &'a [ApprovedMiseRuntime],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedMiseRuntime {
    pub target: ReleaseTarget,
    pub version: String,
    pub sha256: String,
    pub bytes: u64,
}

pub fn verify_lock_against_manifest(
    lock: &GeneratorLock,
    manifest: &ReleaseManifest,
) -> Result<(), LockError> {
    if lock.generator.version != manifest.version {
        return Err(mismatch(format!(
            "version:{}:{}",
            lock.generator.version, manifest.version
        )));
    }
    if lock.generator.commit != manifest.commit {
        return Err(mismatch(format!(
            "commit:{}:{}",
            lock.generator.commit, manifest.commit
        )));
    }
    for target in ReleaseTarget::ALL {
        let locked = lock
            .binary_for_target(target.triple())
            .ok_or_else(|| mismatch(format!("missing_target:{}", target.triple())))?;
        let released = manifest
            .record_for_target(target.triple())
            .ok_or_else(|| mismatch(format!("manifest_missing_target:{}", target.triple())))?;
        if locked.artifact != released.artifact || locked.sha256 != released.sha256 {
            return Err(mismatch(format!("target_diverged:{}", target.triple())));
        }
    }
    Ok(())
}

pub fn verify_lock_against_approved_release(
    lock: &GeneratorLock,
    approved: &ApprovedGeneratorRelease<'_>,
) -> Result<(), LockError> {
    let manifest = approved.manifest;
    manifest
        .validate("published-generator-manifest")
        .map_err(|err| LockError::Invalid {
            problem: err.to_string(),
        })?;
    if lock.generator.binary != "velnor-actions" {
        return Err(mismatch(format!(
            "binary:{}:velnor-actions",
            lock.generator.binary
        )));
    }
    verify_lock_against_manifest(lock, manifest)?;
    verify_approved_mise_runtime(approved)?;
    verify_approved_mise_bootstrap(lock, approved)
}

fn published_verdict_target(target: ReleaseTarget, sha256: &str) -> TargetRecord {
    let target = target.triple();
    TargetRecord {
        target: target.to_owned(),
        artifact: format!(
            "https://github.com/{}/releases/download/v{PUBLISHED_VERDICT_VERSION}/velnor-actions-{PUBLISHED_VERDICT_VERSION}-{target}",
            velnor_actions_contract::targets::EXPECTED_REPOSITORY
        ),
        sha256: sha256.to_owned(),
    }
}

fn verify_approved_mise_bootstrap(
    lock: &GeneratorLock,
    approved: &ApprovedGeneratorRelease<'_>,
) -> Result<(), LockError> {
    if lock.mise_bootstrap.version != approved.mise_bootstrap_version {
        return Err(mismatch(format!(
            "mise_bootstrap:{}:{}",
            lock.mise_bootstrap.version, approved.mise_bootstrap_version
        )));
    }
    if lock.mise_bootstrap.artifact != approved.mise_bootstrap_artifact {
        return Err(mismatch("mise_bootstrap_artifact".to_owned()));
    }
    if lock.mise_bootstrap.sha256 != approved.mise_bootstrap_sha256 {
        return Err(mismatch("mise_bootstrap_sha256".to_owned()));
    }
    Ok(())
}

pub fn catalog_for_approved_generator(
    lock: &GeneratorLock,
    catalog: &ToolCatalog,
    approved: &ApprovedGeneratorRelease<'_>,
) -> Result<ToolCatalog, LockError> {
    verify_lock_against_approved_release(lock, approved)?;
    let mut helper = catalog.clone();
    approved.gh_version.clone_into(&mut helper.gh);
    helper.project.remove(&PinnedTool::Gh);
    Ok(helper)
}

pub fn mise_binary_for_approved_generator<'a>(
    lock: &GeneratorLock,
    target: ReleaseTarget,
    approved: &ApprovedGeneratorRelease<'a>,
) -> Result<super::MiseBinaryIdentity<'a>, LockError> {
    verify_lock_against_approved_release(lock, approved)?;
    let runtime = approved
        .mise_runtime
        .iter()
        .find(|runtime| runtime.target == target)
        .ok_or_else(|| invalid(format!("mise_runtime_missing_target:{}", target.triple())))?;
    Ok(super::MiseBinaryIdentity {
        version: &runtime.version,
        sha256: &runtime.sha256,
        bytes: runtime.bytes,
    })
}

fn verify_approved_mise_runtime(approved: &ApprovedGeneratorRelease<'_>) -> Result<(), LockError> {
    if approved.mise_runtime.len() != ReleaseTarget::ALL.len() {
        return Err(invalid("mise_runtime_missing_target".to_owned()));
    }
    let mut targets = BTreeSet::new();
    let mut version = None;
    for runtime in approved.mise_runtime {
        if !targets.insert(runtime.target) {
            return Err(invalid("mise_runtime_duplicate_target".to_owned()));
        }
        if super::validate_concrete_exact_version("mise", &runtime.version).is_err() {
            return Err(invalid("mise_runtime_malformed_version".to_owned()));
        }
        if let Some(expected) = version {
            if runtime.version.as_str() != expected {
                return Err(invalid("mise_runtime_inconsistent_version".to_owned()));
            }
        } else {
            version = Some(runtime.version.as_str());
        }
        if !is_lower_hex_len(&runtime.sha256, 64) {
            return Err(invalid("mise_runtime_malformed_sha256".to_owned()));
        }
        if runtime.bytes == 0 {
            return Err(invalid("mise_runtime_zero_bytes".to_owned()));
        }
    }
    if ReleaseTarget::ALL
        .iter()
        .any(|target| !targets.contains(target))
    {
        return Err(invalid("mise_runtime_missing_target".to_owned()));
    }
    Ok(())
}

fn invalid(problem: String) -> LockError {
    LockError::Invalid { problem }
}
