//! Stable release recovery-envelope hints; the next job authenticates retained bytes.
use super::super::{
    github::{GITHUB_ACTIONS_BOT_ID, ReleaseResponse, sha256},
    identity::{MAX_RECOVERY_BODY_BYTES, make_recovery_body},
};
use crate::{OrchestratorError, internal::internal};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{
        PackageReleaseChannel, PackageReleaseReceipt, parse_package_release_receipt,
    },
};

/// Parse release metadata only as a hint to locate the original signed artifact.
/// The caller independently authenticates the retained artifact and published release.
pub(super) fn stable_receipt_hint(
    release: &ReleaseResponse,
    plan: &PackageReleasePlan,
    repo_id: u64,
    tag_message: &str,
) -> Result<PackageReleaseReceipt, OrchestratorError> {
    if release.id == 0
        || release.tag_name != format!("v{}", plan.base_version)
        || release.draft != Some(false)
        || release.prerelease != Some(false)
        || release.immutable != Some(true)
        || !release.author.as_ref().is_some_and(|author| {
            author.id == Some(GITHUB_ACTIONS_BOT_ID)
                && author.login.as_deref() == Some("github-actions[bot]")
        })
        || release.body.as_deref() != Some(tag_message)
        || tag_message.len() > MAX_RECOVERY_BODY_BYTES
    {
        return Err(invalid());
    }
    let lines = tag_message.split_terminator('\n').collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != "velnor-package-release-recovery-v1" {
        return Err(invalid());
    }
    let digest = lines[1]
        .strip_prefix("receipt_sha256=")
        .ok_or_else(invalid)?;
    let receipt = URL_SAFE_NO_PAD
        .decode(lines[2].strip_prefix("receipt=").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let bundle = URL_SAFE_NO_PAD
        .decode(lines[3].strip_prefix("bundle=").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if bundle.is_empty()
        || sha256(&receipt) != digest
        || make_recovery_body(&receipt, &bundle, digest)? != tag_message
    {
        return Err(invalid());
    }
    let receipt = parse_package_release_receipt(&receipt).map_err(|_| invalid())?;
    receipt.validate_against_plan(plan).map_err(|_| invalid())?;
    if receipt.channel != PackageReleaseChannel::Stable
        || receipt.repository.slug != plan.repository
        || receipt.repository.id != repo_id
        || receipt.source.default_branch != plan.default_branch
        || receipt.tag != format!("v{}", plan.base_version)
        || receipt.version.base != plan.base_version
        || receipt.version.value != plan.base_version
        || receipt.version.commit_count.is_some()
    {
        return Err(invalid());
    }
    Ok(receipt)
}

fn invalid() -> OrchestratorError {
    internal("tap_continuation_stable_envelope")
}

#[cfg(test)]
#[path = "package_release_continuation_envelope_tests.rs"]
mod tests;
