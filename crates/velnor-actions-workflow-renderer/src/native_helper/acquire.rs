// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{GeneratorLock, ReleaseTarget, Step};

use crate::closure::{HelperProvenance, provision_acquire_step};
use crate::steps::STAGED_BINARY_PREFIX;

use super::ReleaseHelperError;

const GENERATOR_SEED_ROOT: &str = "/opt/velnor/seed";

pub(super) fn published_verdict_acquire_for_target(
    target: ReleaseTarget,
) -> Result<Step, ReleaseHelperError> {
    let manifest = velnor_actions_mise::catalog::lock::published_verdict_manifest();
    manifest
        .validate(velnor_actions_contract::RELEASE_MANIFEST_FILENAME)
        .map_err(ReleaseHelperError::Contract)?;
    let record = manifest.record_for_target(target.triple()).ok_or_else(|| {
        ReleaseHelperError::Problem(format!("manifest_missing_target:{}", target.triple()))
    })?;
    velnor_actions_contract::check_release_artifact(
        &record.artifact,
        &manifest.version,
        &manifest.commit,
        target.triple(),
        velnor_actions_contract::RELEASE_MANIFEST_FILENAME,
        "targets.artifact",
    )
    .map_err(ReleaseHelperError::Contract)?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &manifest.commit,
        &format!("{STAGED_BINARY_PREFIX}{}", manifest.version),
        target,
    )
}

pub(super) fn consumer_acquire_for_target(
    target: ReleaseTarget,
    version: &str,
    json: Option<&str>,
) -> Result<Step, ReleaseHelperError> {
    let manifest = super::consumer_evidence::validated_consumer_manifest(target, version, json)?;
    let record = manifest.record_for_target(target.triple()).ok_or_else(|| {
        ReleaseHelperError::Problem(format!("manifest_missing_target:{}", target.triple()))
    })?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &manifest.commit,
        &format!("{STAGED_BINARY_PREFIX}{version}"),
        target,
    )
}

pub(super) fn lock_acquire_for_target(
    lock: &GeneratorLock,
    target: ReleaseTarget,
    staged: &str,
) -> Result<Step, ReleaseHelperError> {
    let record = lock.binary_for_target(target.triple()).ok_or_else(|| {
        ReleaseHelperError::Problem(format!("lock_missing_target:{}", target.triple()))
    })?;
    acquire_step(
        &record.artifact,
        &record.sha256,
        &lock.generator.commit,
        staged,
        target,
    )
}

fn acquire_step(
    url: &str,
    sha: &str,
    commit: &str,
    staged: &str,
    target: ReleaseTarget,
) -> Result<Step, ReleaseHelperError> {
    let provenance = HelperProvenance::ReleaseAsset {
        url: url.to_owned(),
        sha256: sha.to_owned(),
        commit: commit.to_owned(),
    };
    provision_acquire_step(&provenance, acquire_argv(staged, target)?)
        .map_err(ReleaseHelperError::Render)
}

pub(super) fn acquire_script_argv(
    staged: &str,
    seed_root: &str,
    target: ReleaseTarget,
) -> Result<Vec<String>, ReleaseHelperError> {
    let digest = match target {
        ReleaseTarget::LinuxX86_64 => "sha256sum -c -",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256 -c -",
    };
    if !absolute_token(seed_root) {
        return Err(ReleaseHelperError::Problem(format!(
            "bad_seed_root:{seed_root}"
        )));
    }
    if !staged_path_token(staged) {
        return Err(ReleaseHelperError::Problem(format!(
            "bad_staged_path:{staged}"
        )));
    }
    let name = staged.rsplit_once('/').map_or(staged, |(_, tail)| tail);
    if !file_token(name) {
        return Err(ReleaseHelperError::Problem(format!(
            "bad_staged_name:{name}"
        )));
    }
    let script = format!(
        "d=\"{staged}\"&&mkdir -p \"${{d%/*}}\"&&s=\"{seed_root}/generator/${{d##*/}}\"&&p=\"$VELNOR_ASSET_SHA256  \"&&if [ -f \"$s\" ]&&echo \"$p$s\"|{digest};then cp \"$s\" \"$d\";else curl -fsSL --connect-timeout 15 --max-time 120 --retry 5 --retry-all-errors --retry-max-time 180 --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"$d\"&&echo \"$p$d\"|{digest};fi&&chmod +x \"$d\""
    );
    Ok(vec!["sh".to_owned(), "-c".to_owned(), script])
}

fn acquire_argv(staged: &str, target: ReleaseTarget) -> Result<Vec<String>, ReleaseHelperError> {
    acquire_script_argv(staged, GENERATOR_SEED_ROOT, target)
}

fn absolute_token(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

fn staged_path_token(value: &str) -> bool {
    value
        .strip_prefix(STAGED_BINARY_PREFIX)
        .map_or_else(|| absolute_token(value), file_token)
}

fn file_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
