// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{GeneratorLock, ReleaseManifest, ReleaseTarget, TargetRecord};

use super::{
    MISE_VERSION, PinnedTool, ToolCatalog,
    lock::{LockError, mismatch},
    source_build_bootstrap::{
        SourceBuildBootstrapHost, SourceBuildBootstrapTool, official as bootstrap_asset,
    },
};

const PUBLISHED_GENERATOR_VERSION: &str = "0.1.55";
const PUBLISHED_GENERATOR_GH_VERSION: &str = "2.103.2";
const PUBLISHED_GENERATOR_COMMIT: &str = "693f3aae14406fb7851ac70f0aee647a869b5063";
const PUBLISHED_GENERATOR_LINUX_X64_SHA256: &str =
    "c8f1440e4f767ecc903e6611c56c2db4005011d33ee6f0fd566f4b7e5345eba4";
const PUBLISHED_GENERATOR_MACOS_ARM64_SHA256: &str =
    "eb8690b6a05b18bf025a0d7876c8497cedc04d54df04fdb5a3c20d203478e756";
const PUBLISHED_GENERATOR_MACOS_X64_SHA256: &str =
    "0938a574c00b05a71735285bf0d2840235ca4f9559359909564dd53b27d77883";
const PUBLISHED_VERDICT_VERSION: &str = "0.1.59";
const PUBLISHED_VERDICT_GH_VERSION: &str = "2.103.3";
const PUBLISHED_VERDICT_COMMIT: &str = "4bb724119971fe49f07b12876c54c3c159edb5e5";
const PUBLISHED_VERDICT_LINUX_X64_SHA256: &str =
    "717be00d245f511bd5d324574cb80b72a34c5a83c639fafa6af1c0acb8649b26";
const PUBLISHED_VERDICT_MACOS_ARM64_SHA256: &str =
    "36c47f77001e12c3ea80f4a2559d1b11700935c94488044a966735175b342667";
const PUBLISHED_VERDICT_MACOS_X64_SHA256: &str =
    "20ad2cb971473595d730dd3eb07d8b6bf919f759ffbba2c3b54dca9222a80f7b";

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
    if lock.mise_bootstrap.version != MISE_VERSION {
        return Err(mismatch(format!(
            "mise_bootstrap:{}:{MISE_VERSION}",
            lock.mise_bootstrap.version
        )));
    }
    Ok(())
}

pub fn verify_lock_against_published_release(lock: &GeneratorLock) -> Result<(), LockError> {
    let manifest = published_generator_manifest();
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
    verify_lock_against_manifest(lock, &manifest)?;
    verify_mise_bootstrap(lock)
}

fn published_generator_manifest() -> ReleaseManifest {
    ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: PUBLISHED_GENERATOR_VERSION.to_owned(),
        repository: velnor_actions_contract::targets::EXPECTED_REPOSITORY.to_owned(),
        commit: PUBLISHED_GENERATOR_COMMIT.to_owned(),
        targets: vec![
            published_target(
                ReleaseTarget::LinuxX86_64,
                PUBLISHED_GENERATOR_LINUX_X64_SHA256,
            ),
            published_target(
                ReleaseTarget::MacosArm64,
                PUBLISHED_GENERATOR_MACOS_ARM64_SHA256,
            ),
            published_target(
                ReleaseTarget::MacosX86_64,
                PUBLISHED_GENERATOR_MACOS_X64_SHA256,
            ),
        ],
    }
}

fn published_target(target: ReleaseTarget, sha256: &str) -> TargetRecord {
    let target = target.triple();
    TargetRecord {
        target: target.to_owned(),
        artifact: format!(
            "https://github.com/{}/releases/download/v{PUBLISHED_GENERATOR_VERSION}/velnor-actions-{PUBLISHED_GENERATOR_VERSION}-{target}",
            velnor_actions_contract::targets::EXPECTED_REPOSITORY
        ),
        sha256: sha256.to_owned(),
    }
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

fn verify_mise_bootstrap(lock: &GeneratorLock) -> Result<(), LockError> {
    let released = bootstrap_asset(
        SourceBuildBootstrapTool::Mise,
        SourceBuildBootstrapHost::LinuxAmd64,
    );
    if lock.mise_bootstrap.version != MISE_VERSION {
        return Err(mismatch(format!(
            "mise_bootstrap:{}:{MISE_VERSION}",
            lock.mise_bootstrap.version
        )));
    }
    if lock.mise_bootstrap.artifact != released.asset_url() {
        return Err(mismatch("mise_bootstrap_artifact".to_owned()));
    }
    if lock.mise_bootstrap.sha256 != released.binary_sha256() {
        return Err(mismatch("mise_bootstrap_sha256".to_owned()));
    }
    Ok(())
}

pub fn catalog_for_published_generator(
    lock: &GeneratorLock,
    catalog: &ToolCatalog,
) -> Result<ToolCatalog, LockError> {
    verify_lock_against_published_release(lock)?;
    let mut helper = catalog.clone();
    PUBLISHED_GENERATOR_GH_VERSION.clone_into(&mut helper.gh);
    helper.project.remove(&PinnedTool::Gh);
    Ok(helper)
}

pub fn mise_binary_for_published_generator(
    lock: &GeneratorLock,
    target: ReleaseTarget,
) -> Result<super::MiseBinaryIdentity, LockError> {
    verify_lock_against_published_release(lock)?;
    let (sha256, bytes) = match target {
        ReleaseTarget::LinuxX86_64 => (
            "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
            173_449_632,
        ),
        ReleaseTarget::MacosArm64 => (
            "7dc61dfa71abbcda1c40d18440bc556b5d58b80e6042bc4f95fd106fa1bb3c66",
            138_238_816,
        ),
        ReleaseTarget::MacosX86_64 => (
            "8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8",
            169_788_224,
        ),
    };
    Ok(super::MiseBinaryIdentity {
        version: "2026.10.19",
        sha256,
        bytes,
    })
}
