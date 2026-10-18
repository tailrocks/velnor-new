//! Idempotent Git object, branch, and pull-request operations.

use super::{BotIdentity, TapGitHubApi, encode_segment};
use super::{
    HomebrewTapPreparationProof, OrchestratorError, VerifiedPreviewRelease,
    github::{self, CommitResponse, PullRequestResponse},
    internal,
};
use velnor_actions_contract::config::HomebrewTapConfig;

const TAP_BASE: &str = "main";

pub(super) struct BranchRequest<'a> {
    pub(super) policy: &'a HomebrewTapConfig,
    pub(super) proof: &'a HomebrewTapPreparationProof,
    pub(super) formula: &'a [u8],
    pub(super) base_sha: &'a str,
    pub(super) base_tree_sha: &'a str,
    pub(super) branch: &'a str,
    pub(super) bot: &'a BotIdentity,
    pub(super) verify_current: &'a mut dyn FnMut() -> Result<(), OrchestratorError>,
}

pub(super) fn create_or_reconcile_branch(
    api: &TapGitHubApi,
    request: &mut BranchRequest<'_>,
) -> Result<String, OrchestratorError> {
    (request.verify_current)()?;
    let blob: github::BlobCreateResponse =
        api.post_blob(&api.repo_route("git/blobs"), request.formula)?;
    (request.verify_current)()?;
    let tree: github::TreeCreateResponse = api.post_fields(
        &api.repo_route("git/trees"),
        &[
            ("base_tree", request.base_tree_sha.to_owned()),
            ("tree[][path]", request.policy.formula.clone()),
            ("tree[][mode]", "100644".to_owned()),
            ("tree[][type]", "blob".to_owned()),
            ("tree[][sha]", blob.sha),
        ],
    )?;
    let expected_message = signed_off_message(request.proof, request.bot);
    if let Some(existing_sha) = exact_branch_ref(api, request.branch)? {
        return validate_existing_branch(
            api,
            request.branch,
            &existing_sha,
            request.base_sha,
            &tree.sha,
            &expected_message,
        );
    }
    let email = bot_email(request.bot);
    (request.verify_current)()?;
    let commit: github::CommitCreateResponse = api.post_fields(
        &api.repo_route("git/commits"),
        &[
            ("message", expected_message.clone()),
            ("tree", tree.sha.clone()),
            ("parents[]", request.base_sha.to_owned()),
            ("author[name]", request.bot.login.clone()),
            ("author[email]", email),
            ("committer[name]", request.bot.login.clone()),
            ("committer[email]", bot_email(request.bot)),
        ],
    )?;
    if commit.tree.sha != tree.sha
        || commit.parents.len() != 1
        || commit.parents[0].sha != request.base_sha
        || commit.message != expected_message
    {
        return Err(internal("tap_writer_commit_response_mismatch"));
    }
    (request.verify_current)()?;
    let create_ref = api.post_fields::<serde_json::Value>(
        &api.repo_route("git/refs"),
        &[
            ("ref", format!("refs/heads/{}", request.branch)),
            ("sha", commit.sha.clone()),
        ],
    );
    if create_ref.is_ok() {
        return Ok(commit.sha);
    }
    let raced = exact_branch_ref(api, request.branch)?
        .ok_or_else(|| internal("tap_writer_ref_create_failed"))?;
    validate_existing_branch(
        api,
        request.branch,
        &raced,
        request.base_sha,
        &tree.sha,
        &expected_message,
    )
}

fn exact_branch_ref(api: &TapGitHubApi, branch: &str) -> Result<Option<String>, OrchestratorError> {
    let response: Vec<github::GitRefResponse> =
        api.get_json(&api.repo_route("git/matching-refs/heads/velnor/preview"))?;
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
        "Update preview formula to {}\n\nSource: {}@{}\nRelease receipt: {}\n\nSigned-off-by: {} <{}>",
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
    pub(super) release: &'a VerifiedPreviewRelease,
    pub(super) bot: &'a BotIdentity,
    pub(super) verify_current: &'a mut dyn FnMut() -> Result<(), OrchestratorError>,
}

pub(super) fn create_or_reconcile_pr(
    api: &TapGitHubApi,
    request: &mut PullRequestRequest<'_>,
) -> Result<u64, OrchestratorError> {
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
        return Ok(pull.number);
    }
    let title = expected_pr_title(request.release);
    let body = expected_pr_body(request.release, request.proof_digest, request.base_sha);
    (request.verify_current)()?;
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
    Ok(created.number)
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

pub(super) fn expected_pr_title(release: &VerifiedPreviewRelease) -> String {
    format!(
        "Update preview formula to {}",
        release.receipt.version.value
    )
}

pub(super) fn expected_pr_body(
    release: &VerifiedPreviewRelease,
    proof_digest: &str,
    base_sha: &str,
) -> String {
    format!(
        "Promotes immutable preview release `{}` from `{}` at `{}`.\n\nReceipt digest: `{}`\nCandidate proof digest: `{proof_digest}`\nTap base: `{base_sha}`.",
        release.receipt.tag,
        release.receipt.repository.slug,
        release.receipt.source.commit_sha,
        release.receipt_digest,
    )
}
