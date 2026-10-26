// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BotIdentity, TapGitHubApi, encode_segment};
use super::{
    HomebrewTapPreparationProof, OrchestratorError, VerifiedPackageRelease,
    github::{self, CommitResponse, PullRequestResponse},
    internal,
};
use velnor_actions_contract::config::HomebrewTapConfig;

const TAP_BASE: &str = "main";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CurrentSourceWrite<T> {
    Completed(T),
    SkippedStaleSource,
}

pub(super) struct BranchRequest<'a> {
    pub(super) proof: &'a HomebrewTapPreparationProof,
    pub(super) formula: &'a [u8],
    pub(super) base_sha: &'a str,
    pub(super) base_tree_sha: &'a str,
    pub(super) branch: &'a str,
    pub(super) bot: &'a BotIdentity,
    pub(super) verify_current: &'a mut dyn FnMut() -> Result<bool, OrchestratorError>,
}

pub(super) fn create_or_reconcile_branch(
    api: &TapGitHubApi,
    request: &mut BranchRequest<'_>,
) -> Result<CurrentSourceWrite<String>, OrchestratorError> {
    if !(request.verify_current)()? {
        return Ok(CurrentSourceWrite::SkippedStaleSource);
    }
    let blob: github::BlobCreateResponse =
        api.post_blob(&api.repo_route("git/blobs"), request.formula)?;
    if !(request.verify_current)()? {
        return Ok(CurrentSourceWrite::SkippedStaleSource);
    }
    let tree: github::TreeCreateResponse = api.post_fields(
        &api.repo_route("git/trees"),
        &[
            ("base_tree", request.base_tree_sha.to_owned()),
            ("tree[][path]", request.proof.formula_path.clone()),
            ("tree[][mode]", "100644".to_owned()),
            ("tree[][type]", "blob".to_owned()),
            ("tree[][sha]", blob.sha),
        ],
    )?;
    let expected_message = signed_off_message(request.proof, request.bot);
    if let Some(existing_sha) = exact_branch_ref(api, request.branch, request.proof.channel)? {
        let head = validate_existing_branch(
            api,
            request.branch,
            &existing_sha,
            request.base_sha,
            &tree.sha,
            &expected_message,
        )?;
        return if (request.verify_current)()? {
            Ok(CurrentSourceWrite::Completed(head))
        } else {
            Ok(CurrentSourceWrite::SkippedStaleSource)
        };
    }
    create_branch_commit_and_ref(api, request, &tree.sha, &expected_message)
}

fn create_branch_commit_and_ref(
    api: &TapGitHubApi,
    request: &mut BranchRequest<'_>,
    tree_sha: &str,
    expected_message: &str,
) -> Result<CurrentSourceWrite<String>, OrchestratorError> {
    let email = bot_email(request.bot);
    if !(request.verify_current)()? {
        return Ok(CurrentSourceWrite::SkippedStaleSource);
    }
    let commit: github::CommitCreateResponse = api.post_fields(
        &api.repo_route("git/commits"),
        &[
            ("message", expected_message.to_owned()),
            ("tree", tree_sha.to_owned()),
            ("parents[]", request.base_sha.to_owned()),
            ("author[name]", request.bot.login.clone()),
            ("author[email]", email),
            ("committer[name]", request.bot.login.clone()),
            ("committer[email]", bot_email(request.bot)),
        ],
    )?;
    if commit.tree.sha != tree_sha
        || commit.parents.len() != 1
        || commit.parents[0].sha != request.base_sha
        || commit.message != expected_message
    {
        return Err(internal("tap_writer_commit_response_mismatch"));
    }
    if !(request.verify_current)()? {
        return Ok(CurrentSourceWrite::SkippedStaleSource);
    }
    let create_ref = api.post_fields::<serde_json::Value>(
        &api.repo_route("git/refs"),
        &[
            ("ref", format!("refs/heads/{}", request.branch)),
            ("sha", commit.sha.clone()),
        ],
    );
    if create_ref.is_ok() {
        return if (request.verify_current)()? {
            Ok(CurrentSourceWrite::Completed(commit.sha))
        } else {
            Ok(CurrentSourceWrite::SkippedStaleSource)
        };
    }
    let raced = exact_branch_ref(api, request.branch, request.proof.channel)?
        .ok_or_else(|| internal("tap_writer_ref_create_failed"))?;
    let head = validate_existing_branch(
        api,
        request.branch,
        &raced,
        request.base_sha,
        tree_sha,
        expected_message,
    )?;
    if (request.verify_current)()? {
        Ok(CurrentSourceWrite::Completed(head))
    } else {
        Ok(CurrentSourceWrite::SkippedStaleSource)
    }
}

fn exact_branch_ref(
    api: &TapGitHubApi,
    branch: &str,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<Option<String>, OrchestratorError> {
    let response: Vec<github::GitRefResponse> = api.get_json(&api.repo_route(&format!(
        "git/matching-refs/heads/velnor/{}",
        channel_key(channel),
    )))?;
    let expected = format!("refs/heads/{branch}");
    let mut found = response
        .into_iter()
        .filter(|entry| entry.ref_name == expected);
    let Some(entry) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() || entry.object.object_type != "commit" {
        return Err(internal("tap_writer_duplicate_branch_ref"));
    }
    Ok(Some(entry.object.sha))
}

fn validate_existing_branch(
    api: &TapGitHubApi,
    _branch: &str,
    commit_sha: &str,
    base_sha: &str,
    expected_tree_sha: &str,
    expected_message: &str,
) -> Result<String, OrchestratorError> {
    let commit: CommitResponse =
        api.get_json(&api.repo_route(&format!("git/commits/{commit_sha}")))?;
    if commit.sha != commit_sha
        || commit.tree.sha != expected_tree_sha
        || commit.parents.len() != 1
        || commit.parents[0].sha != base_sha
        || commit.message != expected_message
    {
        return Err(internal("tap_writer_conflicting_branch_retry"));
    }
    Ok(commit.sha)
}

pub(super) fn signed_off_message(proof: &HomebrewTapPreparationProof, bot: &BotIdentity) -> String {
    format!(
        "Update {} formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}>",
        channel_key(proof.channel),
        proof.version,
        proof.source_repository,
        proof.source_sha,
        proof.receipt_digest,
        bot.login,
        bot_email(bot),
    )
}

fn bot_email(bot: &BotIdentity) -> String {
    format!("{}+{}@users.noreply.github.com", bot.user_id, bot.login)
}

pub(super) struct PullRequestRequest<'a> {
    pub(super) policy: &'a HomebrewTapConfig,
    pub(super) branch: &'a str,
    pub(super) head_sha: &'a str,
    pub(super) base_sha: &'a str,
    pub(super) proof_digest: &'a str,
    pub(super) release: &'a VerifiedPackageRelease,
    pub(super) bot: &'a BotIdentity,
    pub(super) verify_current: &'a mut dyn FnMut() -> Result<bool, OrchestratorError>,
}

pub(super) fn create_or_reconcile_pr(
    api: &TapGitHubApi,
    request: &mut PullRequestRequest<'_>,
) -> Result<CurrentSourceWrite<u64>, OrchestratorError> {
    let policy = request.policy;
    let branch = request.branch;
    let owner = policy
        .repository
        .split_once('/')
        .map(|(owner, _)| owner)
        .ok_or_else(|| internal("tap_writer_repository_slug_invalid"))?;
    let query = format!(
        "{}?head={}:{}&base={TAP_BASE}&state=all&per_page=100",
        api.repo_route("pulls"),
        owner,
        encode_segment(branch),
    );
    let existing: Vec<PullRequestResponse> = api.get_json(&query)?;
    let matching: Vec<_> = existing
        .into_iter()
        .filter(|pull| pull.head.ref_name == branch)
        .collect();
    if matching.len() > 1 {
        return Err(internal("tap_writer_duplicate_pull_request"));
    }
    if let Some(pull) = matching.first() {
        if !matches_expected_pr(pull, request, owner) {
            return Err(internal("tap_writer_conflicting_pull_request_retry"));
        }
        return if (request.verify_current)()? {
            Ok(CurrentSourceWrite::Completed(pull.number))
        } else {
            Ok(CurrentSourceWrite::SkippedStaleSource)
        };
    }
    let title = expected_pr_title(request.release);
    let body = expected_pr_body(request.release, request.proof_digest, request.base_sha);
    if !(request.verify_current)()? {
        return Ok(CurrentSourceWrite::SkippedStaleSource);
    }
    let created: PullRequestResponse = api.post_fields(
        &api.repo_route("pulls"),
        &[
            ("title", title.clone()),
            ("head", format!("{owner}:{branch}")),
            ("base", TAP_BASE.to_owned()),
            ("body", body.clone()),
            ("draft", "false".to_owned()),
        ],
    )?;
    if !matches_expected_pr(&created, request, owner) {
        return Err(internal("tap_writer_pull_request_response_mismatch"));
    }
    if (request.verify_current)()? {
        Ok(CurrentSourceWrite::Completed(created.number))
    } else {
        Ok(CurrentSourceWrite::SkippedStaleSource)
    }
}

fn matches_expected_pr(
    pull: &PullRequestResponse,
    request: &PullRequestRequest<'_>,
    owner: &str,
) -> bool {
    pull.head.sha == request.head_sha
        && pull.head.label == format!("{owner}:{}", request.branch)
        && pull.base.ref_name == TAP_BASE
        && pull.base.sha == request.base_sha
        && pull.state == "open"
        && pull.merged != Some(true)
        && pull.user.id == request.bot.user_id
        && pull.user.login == request.bot.login
        && pull
            .head
            .repo
            .as_ref()
            .is_some_and(|repo| repo.full_name == request.policy.repository)
        && pull
            .base
            .repo
            .as_ref()
            .is_some_and(|repo| repo.full_name == request.policy.repository)
        && pull.title == expected_pr_title(request.release)
        && pull.body.as_deref()
            == Some(
                expected_pr_body(request.release, request.proof_digest, request.base_sha).as_str(),
            )
}

pub(super) fn expected_pr_title(release: &VerifiedPackageRelease) -> String {
    format!(
        "Update {} formula to {}",
        channel_key(release.receipt.channel),
        release.receipt.version.value
    )
}

pub(super) fn expected_pr_body(
    release: &VerifiedPackageRelease,
    proof_digest: &str,
    base_sha: &str,
) -> String {
    format!(
        "Promotes {} release `{}` from `{}` at `{}`.\n\nReceipt digest: `{}`\nCandidate proof digest: `{proof_digest}`\nTap base: `{base_sha}`.",
        match release.receipt.channel {
            velnor_actions_contract::package_release::PackageReleaseChannel::Preview =>
                "rolling preview",
            velnor_actions_contract::package_release::PackageReleaseChannel::Stable =>
                "immutable stable",
        },
        release.receipt.tag,
        release.receipt.repository.slug,
        release.receipt.source.commit_sha,
        release.receipt_digest,
    )
}

fn channel_key(
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> &'static str {
    match channel {
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
    }
}
