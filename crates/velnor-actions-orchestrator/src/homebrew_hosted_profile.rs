use std::{path::PathBuf, time::Duration};

use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError,
    homebrew::install::{HOMEBREW_EXECUTABLE, HOMEBREW_VERSION},
    internal::internal,
};

pub(super) const BREW_PINNED_COMMIT: &str = "7a056ffd71e505f725b092d162fb09164a62b510";
pub(super) const BREW_PINNED_TREE: &str = "e8a495cf3247a4af10093b5460e074d27d47999f";
pub(super) const BREW_DESCRIBE: &str = "7.0.10-84-g7a056ff";
pub(super) const BREW_TAG_COMMIT: &str = "8f10429b56be28a25032b87463fffc95e556b5bc";
pub(super) const PORTABLE_RUBY_VERSION: &str = "4.0.9";
pub(super) const PORTABLE_RUBY_ARCHIVE_SHA256: &str =
    "43ff285a82f48c84a06d0676a3800815a19d290d32e44182ddd7a6e95478e18e";
const SETUP_LIMIT: Duration = Duration::from_mins(5);
pub(super) const COMMAND_LIMIT: Duration = Duration::from_secs(120);
pub(super) const CAPTURE_LIMIT: usize = 128 * 1024;
pub(super) const RUBY_BINARY_LIMIT: u64 = 128 * 1024 * 1024;
pub(super) const HOMEBREW_PREFIX: &str = "/opt/homebrew";

#[path = "homebrew_hosted_profile_activation.rs"]
mod activation;
#[path = "homebrew_hosted_profile_fs.rs"]
mod profile_fs;
#[path = "homebrew_hosted_profile_support.rs"]
mod support;
use activation::prepare_within;
pub(crate) use support::verify_release_qualification_candidate;
use support::{is_b3_digest, is_git_sha1, is_sha256, validate_host_identity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostedHomebrewProfile {
    pub(crate) prefix: PathBuf,
    pub(crate) previous_prefix: PathBuf,
    pub(crate) previous_prefix_device: u64,
    pub(crate) previous_prefix_inode: u64,
    pub(crate) active_prefix_device: u64,
    pub(crate) active_prefix_inode: u64,
    pub(crate) source_commit: String,
    pub(crate) source_tree: String,
    pub(crate) source_describe: String,
    pub(crate) source_tag_commit: String,
    pub(crate) portable_ruby_version: String,
    pub(crate) portable_ruby_path: PathBuf,
    pub(crate) portable_ruby_archive_sha256: String,
    pub(crate) portable_ruby_binary_sha256: String,
}

#[derive(Debug)]
pub(crate) struct VerifiedReleaseQualificationCandidate {
    manifest_sha256: String,
    target_sha256: String,
}

pub(crate) fn prepare_for_release_qualification(
    candidate: &VerifiedReleaseQualificationCandidate,
    deadline: CheckDeadline,
) -> Result<HostedHomebrewProfile, OrchestratorError> {
    if !is_sha256(&candidate.manifest_sha256) || !is_sha256(&candidate.target_sha256) {
        return Err(internal("homebrew_profile_candidate_custody"));
    }
    prepare_profile(deadline)
}

pub(crate) fn prepare_hosted_homebrew(
    custody: &crate::homebrew::runtime::VerifiedNativeHomebrewInstall,
    deadline: CheckDeadline,
) -> Result<HostedHomebrewProfile, OrchestratorError> {
    let _channel = custody.channel();
    if custody.operation() != velnor_actions_contract::config::NativeHomebrewOperation::Install
        || custody.platform() != velnor_actions_contract::config::CheckPlatform::MacosArm64
        || custody.tap_repository_id() == 0
        || custody.tap_repository().is_empty()
        || custody.run_key().is_empty()
        || !is_git_sha1(custody.head())
        || custody.check_id().is_empty()
        || custody.task_id().is_empty()
        || custody.plan_id().is_empty()
        || !is_b3_digest(custody.entry_digest())
        || !is_b3_digest(custody.task_digest())
        || !is_b3_digest(custody.input_digest())
        || !is_b3_digest(custody.closure_digest())
        || !is_git_sha1(custody.tap_base_sha())
        || custody.formula_path().is_empty()
        || !is_git_sha1(custody.formula_blob_sha())
    {
        return Err(internal("homebrew_profile_native_custody"));
    }
    prepare_profile(deadline)
}

fn prepare_profile(deadline: CheckDeadline) -> Result<HostedHomebrewProfile, OrchestratorError> {
    validate_host_identity(&|key| std::env::var(key))
        .map_err(|()| internal("homebrew_profile_host_identity"))?;
    let setup_deadline = CheckDeadline::after(SETUP_LIMIT)
        .map_err(|_| internal("homebrew_profile_setup_deadline"))?;
    deadline
        .scope(|| setup_deadline.scope(|| prepare_within(setup_deadline)))
        .map_err(|_| internal("homebrew_profile_deadline"))?
        .map_err(|_| internal("homebrew_profile_deadline"))?
}
