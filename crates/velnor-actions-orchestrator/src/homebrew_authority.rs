// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, WorkflowEvent,
    config::{HomebrewTapConfig, RustBinaryReleaseArchive},
    homebrew_authority::{
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH, HOMEBREW_SOURCE_CONFIG_PATH,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH, HomebrewSourceAuthorityLocator,
        MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES, parse_homebrew_source_authority_locator,
    },
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "homebrew_authority/context.rs"]
mod context;
#[path = "homebrew_authority/source.rs"]
mod source;
use source::{SourceProjection, load_source_projection};
#[path = "homebrew_authority/applicability.rs"]
mod applicability;

const SOURCE_DOCUMENT_LIMIT: usize =
    velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES;

#[derive(Debug, Clone)]
pub(crate) struct VerifiedHomebrewAuthority {
    source_config: velnor_actions_contract::config::VelnorConfig,
    spec: velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    tap: HomebrewTapConfig,
    plan: PackageReleasePlan,
    source_repository_id: u64,
    authority_sha: String,
    tap_repository: String,
    tap_repository_id: u64,
    tap_base_sha: String,
    tap_head_sha: String,
    locator_blob_sha: String,
}

impl VerifiedHomebrewAuthority {
    pub(crate) fn source_config(&self) -> &velnor_actions_contract::config::VelnorConfig {
        &self.source_config
    }

    pub(crate) fn spec(&self) -> &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec {
        &self.spec
    }

    pub(crate) fn tap(&self) -> &HomebrewTapConfig {
        &self.tap
    }

    pub(crate) fn plan(&self) -> &PackageReleasePlan {
        &self.plan
    }

    pub(crate) const fn source_repository_id(&self) -> u64 {
        self.source_repository_id
    }

    pub(crate) fn authority_sha(&self) -> &str {
        &self.authority_sha
    }

    pub(crate) const fn tap_repository_id(&self) -> u64 {
        self.tap_repository_id
    }

    pub(crate) fn tap_base_sha(&self) -> &str {
        &self.tap_base_sha
    }

    pub(crate) fn tap_head_sha(&self) -> &str {
        &self.tap_head_sha
    }

    pub(crate) fn locator_blob_sha(&self) -> &str {
        &self.locator_blob_sha
    }

    pub(crate) fn protected_tap_file(
        &self,
        path: &str,
        max_bytes: usize,
        deadline: CheckDeadline,
    ) -> Result<Vec<u8>, OrchestratorError> {
        if !safe_source_path(path) || max_bytes == 0 || max_bytes > SOURCE_DOCUMENT_LIMIT {
            return Err(internal("homebrew_authority_tap_file_selection"));
        }
        crate::package_release_publish::read_protected_source_file(
            &self.tap_repository,
            self.tap_repository_id,
            "main",
            &self.tap_base_sha,
            path,
            max_bytes,
            deadline,
        )
    }

    pub(crate) fn public_policy(
        &self,
    ) -> Result<crate::package_release_public::PackageReleasePublicPolicy, OrchestratorError> {
        crate::package_release_public::PackageReleasePublicPolicy::from_trusted_source_authority(
            self.plan.clone(),
            self.source_repository_id,
            self.authority_sha.clone(),
        )
    }
}

pub(crate) fn load_protected_homebrew_authority(
    root: &Path,
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: CheckDeadline,
) -> Result<VerifiedHomebrewAuthority, OrchestratorError> {
    probe_protected_homebrew_authority(
        root,
        expected_tap_repository,
        event,
        pull_request,
        expected_head,
        deadline,
    )?
    .ok_or_else(|| internal("homebrew_authority_locator_missing"))
}

pub(crate) fn probe_protected_homebrew_authority(
    root: &Path,
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: CheckDeadline,
) -> Result<Option<VerifiedHomebrewAuthority>, OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(expected_tap_repository)
        || !lowercase_sha(expected_head)
    {
        return Err(internal("homebrew_authority_selection_invalid"));
    }

    let tap_context = context::authenticate_tap_context(
        root,
        expected_tap_repository,
        event,
        pull_request,
        expected_head,
        deadline,
    )?;
    let context::TapContext {
        tap_repository_id,
        tap_base_sha,
        authenticated_head,
        ..
    } = &tap_context;
    let Some(locator_bytes) = protected_locator(
        expected_tap_repository,
        *tap_repository_id,
        tap_base_sha,
        deadline,
    )?
    else {
        return Ok(None);
    };
    let locator_blob_sha =
        crate::package_release_publish::release_policy_admission::git_blob_sha1(&locator_bytes)?;
    let locator = parse_homebrew_source_authority_locator(&locator_bytes)
        .map_err(|_| internal("homebrew_authority_locator_invalid"))?;
    if locator.default_branch != "main" {
        return Err(internal("homebrew_authority_source_branch"));
    }

    let SourceProjection {
        source_tip,
        source_config,
        plan,
        tap,
        spec,
    } = load_source_projection(&locator, expected_tap_repository, deadline)?;

    context::recheck_tap_context(
        root,
        expected_tap_repository,
        event,
        pull_request,
        expected_head,
        &tap_context,
        deadline,
    )?;

    let checkout_head = read_git_value(root, "HEAD", deadline)?;
    if checkout_head != *authenticated_head {
        return Err(internal("homebrew_authority_checkout_head"));
    }

    Ok(Some(VerifiedHomebrewAuthority {
        source_config,
        spec,
        tap,
        plan,
        source_repository_id: locator.repository_id,
        authority_sha: source_tip,
        tap_repository: expected_tap_repository.to_owned(),
        tap_repository_id: *tap_repository_id,
        tap_base_sha: tap_base_sha.clone(),
        tap_head_sha: authenticated_head.clone(),
        locator_blob_sha,
    }))
}

pub(crate) fn protected_locator_absent_on_default_branch(
    repository: &str,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let before =
        crate::package_release_publish::read_public_repository_snapshot(repository, deadline)?;
    let before_tip = crate::package_release_publish::read_public_branch_tip(
        repository,
        before.id,
        &before.default_branch,
        deadline,
    )?;
    let locator = crate::package_release_publish::try_read_protected_source_file(
        repository,
        before.id,
        &before.default_branch,
        &before_tip,
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH,
        MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES,
        deadline,
    )?;
    let after_tip = crate::package_release_publish::read_public_branch_tip(
        repository,
        before.id,
        &before.default_branch,
        deadline,
    )?;
    let after =
        crate::package_release_publish::read_public_repository_snapshot(repository, deadline)?;
    applicability::confirm_locator_absence(
        repository,
        &before,
        &before_tip,
        locator.as_deref(),
        &after,
        &after_tip,
    )
}

fn read_git_value(
    root: &Path,
    revision: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let result =
        velnor_actions_mise::GitRequest::rev_parse(vec![std::ffi::OsString::from(revision)])
            .run_in_until(root, deadline)
            .map_err(|_| internal("homebrew_authority_git_context"))?;
    result
        .require_success("git")
        .map_err(|_| internal("homebrew_authority_git_context"))?;
    let value = result
        .stdout_text("git")
        .map_err(|_| internal("homebrew_authority_git_context"))?;
    let value = value.trim();
    if !lowercase_sha(value) {
        return Err(internal("homebrew_authority_git_context"));
    }
    Ok(value.to_owned())
}

fn lowercase_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_source_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn admit_pull_request_context(
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    event: WorkflowEvent,
    repository: &str,
    repository_id: u64,
    expected_head: &str,
    checkout_head: &str,
    runner: &context::pull_request_context::RunnerFacts,
) -> Result<(String, String), OrchestratorError> {
    if !matches!(event, WorkflowEvent::PullRequest | WorkflowEvent::Fork) {
        return Err(internal("homebrew_authority_pull_request_event"));
    }
    let fork = event == WorkflowEvent::Fork;
    if pull.state != "open"
        || pull.draft
        || pull.merged
        || pull.base_repository != repository
        || pull.base_repository_id != repository_id
        || pull.base_ref != "main"
        || runner.payload_base_repository != repository
        || runner.payload_base_repository_id != repository_id
        || runner.payload_base_ref != "main"
        || (fork
            && (pull.head_repository == repository || pull.head_repository_id == repository_id))
        || (!fork
            && (pull.head_repository != repository || pull.head_repository_id != repository_id))
        || pull.base_sha != runner.payload_base_sha
        || pull.head_sha != runner.payload_head_sha
        || pull.head_repository != runner.payload_head_repository
        || pull.head_repository_id != runner.payload_head_repository_id
        || pull.head_ref != runner.payload_head_ref
        || runner.payload_repository_id != repository_id
        || (runner.sha != pull.head_sha
            && pull.merge_commit_sha.as_deref() != Some(runner.sha.as_str()))
        || (expected_head != pull.head_sha && expected_head != checkout_head)
        || (checkout_head != pull.head_sha
            && pull.merge_commit_sha.as_deref() != Some(checkout_head))
    {
        return Err(internal("homebrew_authority_pull_request_context"));
    }
    Ok((pull.base_sha.clone(), pull.head_sha.clone()))
}

fn protected_locator(
    expected_tap_repository: &str,
    tap_repository_id: u64,
    tap_base_sha: &str,
    deadline: CheckDeadline,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    crate::package_release_publish::try_read_protected_source_file(
        expected_tap_repository,
        tap_repository_id,
        "main",
        tap_base_sha,
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH,
        MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES,
        deadline,
    )
}
