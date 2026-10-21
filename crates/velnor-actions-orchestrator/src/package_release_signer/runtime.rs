//! Identity, version, and filesystem helpers used by the native release signer.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    PackageReleasePlan,
    config::RustBinaryReleaseCompanion,
    package_release::{PackageReleaseChannel, PackageReleaseEvent, PackageReleaseReceipt},
    preview_identity::PreviewIdentity,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use super::PackageReleaseSigningIdentity;
use crate::{OrchestratorError, internal::internal};

pub(super) fn signing_identity_from_env(
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let started = std::time::Instant::now();
    let repository = required_env("GITHUB_REPOSITORY")?;
    let repository_id = query_repository_id(&repository)?;
    signing_identity_for_repository(plan, repository_id, started)
}

pub(super) fn signing_identity_for_repository(
    plan: &PackageReleasePlan,
    repository_id: u64,
    started: std::time::Instant,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let identity = identity_for_repository(plan, repository_id)?;
    if plan.channel == PackageReleaseChannel::Stable {
        let deadline = velnor_actions_mise::CheckDeadline::from_start_with_cleanup_reserve(
            started,
            Duration::from_secs(
                u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
            ),
        )
        .map_err(|_| internal("package_signer_deadline"))?;
        let admitted = crate::stable_tag_origin::admitted_production_context(plan, deadline)?;
        require_stable_context(plan, &identity, &admitted)?;
    }
    Ok(identity)
}

// Published reads authenticate the exact current Tap caller and remote stable
// origin downstream; they never acquire signing authority from this identity.
pub(super) fn published_identity_from_env(
    plan: &PackageReleasePlan,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let repository_id = query_repository_id(&required_env("GITHUB_REPOSITORY")?)?;
    identity_for_repository(plan, repository_id)
}

fn identity_for_repository(
    plan: &PackageReleasePlan,
    repository_id: u64,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let repository = required_env("GITHUB_REPOSITORY")?;
    let source_sha = required_env("GITHUB_SHA")?;
    let source_ref = required_env("GITHUB_REF")?;
    let workflow_ref = required_env("GITHUB_WORKFLOW_REF")?;
    let workflow_sha = required_env("GITHUB_WORKFLOW_SHA")?;
    let event = signing_event(plan, &required_env("GITHUB_EVENT_NAME")?)?;
    let run_id = positive_u64("GITHUB_RUN_ID")?;
    let run_attempt = positive_u32("GITHUB_RUN_ATTEMPT")?;
    let ci_run_id = positive_u64("VELNOR_RELEASE_CI_RUN_ID")?;
    let ci_run_attempt = positive_u32("VELNOR_RELEASE_CI_RUN_ATTEMPT")?;
    let commit_count = match plan.channel {
        PackageReleaseChannel::Preview => Some(positive_u64("VELNOR_RELEASE_COMMIT_COUNT")?),
        PackageReleaseChannel::Stable => None,
    };
    let identity = PackageReleaseSigningIdentity {
        repository,
        repository_id,
        source_sha,
        source_ref,
        default_branch: plan.default_branch.clone(),
        event,
        workflow_path: plan.channel.workflow_path().to_owned(),
        workflow_ref,
        workflow_sha,
        run_id,
        run_attempt,
        ci_run_id,
        ci_run_attempt,
        commit_count,
    };
    validate_identity(plan, &identity)?;
    Ok(identity)
}

pub(super) fn require_stable_context(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
    admitted: &velnor_actions_contract::preview_production::ReleaseProductionContext,
) -> Result<(), OrchestratorError> {
    if plan.channel != PackageReleaseChannel::Stable
        || admitted.repository_id != identity.repository_id
        || admitted.head_repository_id != identity.repository_id
        || admitted.source_sha != identity.source_sha
        || admitted.source_ref != identity.source_ref
        || admitted.workflow_authority_sha != identity.workflow_sha
        || admitted.run_id != identity.run_id
        || admitted.run_attempt != identity.run_attempt
        || admitted.ci_run_id != identity.ci_run_id
        || admitted.ci_run_attempt != identity.ci_run_attempt
        || admitted.commit_count != 0
        || admitted.version != plan.base_version
    {
        return Err(internal("package_signer_stable_origin_context_mismatch"));
    }
    Ok(())
}

pub(super) fn signing_event(
    plan: &PackageReleasePlan,
    actual: &str,
) -> Result<PackageReleaseEvent, OrchestratorError> {
    let expected = plan.channel.event();
    if actual != expected.as_str() {
        return Err(internal("package_signer_event_invalid"));
    }
    Ok(expected)
}

pub(super) fn validate_identity(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
) -> Result<(), OrchestratorError> {
    let workflow_ref = format!(
        "{}/{}@{}",
        identity.repository, identity.workflow_path, identity.source_ref
    );
    if identity.repository != plan.repository
        || identity.repository_id == 0
        || !is_full_sha(&identity.source_sha)
        || identity.workflow_sha != identity.source_sha
        || identity.source_ref != format!("refs/heads/{}", plan.default_branch)
        || identity.workflow_path != plan.channel.workflow_path()
        || identity.workflow_ref != workflow_ref
        || identity.run_id == 0
        || identity.run_attempt == 0
        || identity.ci_run_id == 0
        || identity.ci_run_attempt == 0
        || match plan.channel {
            PackageReleaseChannel::Preview => {
                identity.event != plan.channel.event()
                    || identity.commit_count.is_none_or(|count| count == 0)
            }
            PackageReleaseChannel::Stable => {
                identity.event != plan.channel.event() || identity.commit_count.is_some()
            }
        }
    {
        return Err(internal("package_signer_workflow_identity_mismatch"));
    }
    Ok(())
}

pub(super) fn release_version(
    plan: &PackageReleasePlan,
    identity: &PackageReleaseSigningIdentity,
) -> Result<(String, String, Option<u64>, Option<String>), OrchestratorError> {
    match plan.channel {
        PackageReleaseChannel::Preview => {
            let version = PreviewIdentity::derive(
                &plan.base_version,
                &identity.source_sha,
                identity
                    .commit_count
                    .ok_or_else(|| internal("package_signer_commit_count_missing"))?,
            )
            .map_err(|_| internal("package_signer_preview_version_invalid"))?;
            Ok((
                version.version,
                version.tag,
                Some(version.commit_count),
                Some(identity.source_sha[..7].to_owned()),
            ))
        }
        PackageReleaseChannel::Stable => Ok((
            plan.base_version.clone(),
            format!("v{}", plan.base_version),
            None,
            None,
        )),
    }
}

pub(super) fn release_companions(plan: &PackageReleasePlan) -> Vec<RustBinaryReleaseCompanion> {
    plan.companions
        .iter()
        .map(|companion| RustBinaryReleaseCompanion {
            id: companion.id.clone(),
            path: companion.path.clone(),
            asset_name: companion.asset_name.clone(),
        })
        .collect()
}

pub(super) fn query_repository_id(repository: &str) -> Result<u64, OrchestratorError> {
    let token = env::var("GH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| internal("package_signer_read_token_missing"))?;
    let api_path = format!("repos/{repository}");
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        std::ffi::OsStr::new("gh"),
        ["api", &api_path, "--jq", ".id"]
            .into_iter()
            .map(Into::into)
            .collect(),
    )
    .map_err(|_| internal("package_signer_gh_request_invalid"))?;
    let command = request
        .command_with_github_token(&ToolCatalog::pinned(), &token)
        .map_err(|_| internal("package_signer_gh_request_invalid"))?;
    let output = command
        .run_bounded(1024, Duration::from_secs(30))
        .map_err(|_| internal("package_signer_repository_lookup_failed"))?;
    if !output.success {
        return Err(internal("package_signer_repository_lookup_failed"));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| internal("package_signer_repository_id_invalid"))?
        .trim();
    text.parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("package_signer_repository_id_invalid"))
}

pub(super) fn validate_release_directory(
    directory: &Path,
    receipt: &PackageReleaseReceipt,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| internal("package_release_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_release_directory_invalid"));
    }
    let expected = receipt
        .assets
        .iter()
        .map(|asset| asset.name.as_str())
        .chain(["release.json", "release.json.bundle"])
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    for entry in
        fs::read_dir(directory).map_err(|_| internal("package_release_directory_read_failed"))?
    {
        let entry = entry.map_err(|_| internal("package_release_directory_read_failed"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| internal("package_release_asset_name_invalid"))?;
        if !expected.contains(name.as_str()) || observed.len() >= expected.len() {
            return Err(internal("package_release_directory_inventory_mismatch"));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| internal("package_release_asset_invalid"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || !observed.insert(name) {
            return Err(internal("package_release_directory_entry_invalid"));
        }
    }
    if observed.len() != expected.len() || !expected.iter().all(|name| observed.contains(*name)) {
        return Err(internal("package_release_directory_inventory_mismatch"));
    }
    Ok(())
}

pub(super) fn read_regular_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_signer_file_missing"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err(internal("package_signer_file_invalid"));
    }
    fs::read(path).map_err(|_| internal("package_signer_file_read_failed"))
}

pub(super) fn require_private_directory(path: &Path) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_signer_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_signer_directory_invalid"));
    }
    Ok(())
}

pub(super) fn absolute_env(name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required_env(name)?);
    if !path.is_absolute() {
        return Err(internal("package_signer_path_not_absolute"));
    }
    Ok(path)
}

pub(super) fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("package_signer_context_missing"))
}

pub(super) fn positive_u64(name: &str) -> Result<u64, OrchestratorError> {
    required_env(name)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_signer_run_identity_invalid"))
}

pub(super) fn positive_u32(name: &str) -> Result<u32, OrchestratorError> {
    required_env(name)?
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_signer_run_identity_invalid"))
}

pub(super) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| internal("package_signer_create_failed"))?;
    file.write_all(bytes)
        .map_err(|_| internal("package_signer_write_failed"))?;
    file.sync_all()
        .map_err(|_| internal("package_signer_sync_failed"))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}
