//! Two-phase, source-bound preview admission. Phase one authenticates remote
//! CI and workflow identities; phase two reads only regular files from the
//! pinned source checkout and emits the immutable admission record.

use super::{
    Admission, append_output, positive_u64, required, runner_root, source_files::SourceFiles,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use velnor_actions_contract::{
    PackageReleasePolicy, config::RustBinaryReleaseConfig, package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

use super::super::{
    github::GitHubApi,
    run_identity::{CurrentRun, RunSource},
};

/// Native operation that finalizes the selected preview source admission.
pub const FINALIZE_PREVIEW_SOURCE_OP: &str = "finalize-preview-source-v1";

/// Select source A and current workflow authority B from authenticated GitHub facts.
/// # Errors
/// Rejects an ineligible CI source, workflow authority, repository, or ancestry.
pub fn select_preview_source_from_env() -> Result<(), OrchestratorError> {
    select()
}

/// Finalize A's static policy, task closure, and digest-bound source artifact.
/// # Errors
/// Rejects checkout, policy, producer-task, source-closure, or authentication drift.
pub fn finalize_preview_source_from_env() -> Result<(), OrchestratorError> {
    finalize()
}

const ADMISSION_FILE: &str = "preview-source-admission.json";
const SOURCE_SEED_POLICY_FILE: &str = "preview-source-seed-policy.json";
const MAX_ADMISSION_BYTES: usize = 16 * 1024 * 1024;
const MAX_MANIFEST_CLOSURE_BYTES: usize = 8 * 1024 * 1024;

type SourceEntryMap = BTreeMap<String, super::source_files::SourceFileEntry>;

#[path = "preview_source_admission_finalize.rs"]
mod finalize;
pub(super) use finalize::finalize;
#[path = "preview_source_admission_projection.rs"]
mod projection;

struct Finalization {
    seed: PackageReleasePolicy,
    source_sha: String,
    authority: String,
    ci_run_id: u64,
    ci_attempt: u32,
    count: u64,
    repository_id: u64,
    run_id: u64,
    run_attempt: u32,
    workspace: PathBuf,
    api: GitHubApi,
    release: RustBinaryReleaseConfig,
    qualified_tools: Vec<velnor_actions_contract::config::QualifiedTool>,
}

struct WorkspaceSource<'a> {
    record: velnor_actions_rust::WorkspaceRecord,
    reader: SourceFiles<'a>,
    manifests: BTreeMap<String, Vec<u8>>,
}

/// Authenticate a successful main CI source and the current workflow authority.
/// This phase runs before checkout and emits only remote identities.
pub(super) fn select() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("preview_source_deadline"))?;
    deadline
        .scope(|| select_until(deadline))
        .map_err(|_| internal("preview_source_deadline"))?
}

fn select_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let seed = parse_seed_policy()?;
    let repository = required("GITHUB_REPOSITORY")?;
    let coordinator = required("GITHUB_SHA")?;
    if seed.repository != repository
        || seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || coordinator != required("GITHUB_WORKFLOW_SHA")?
        || !valid_git_sha(&coordinator)
        || required("GITHUB_REF")? != "refs/heads/main"
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{repository}/{}@refs/heads/main",
                seed.channel.workflow_path()
            )
    {
        return Err(internal("preview_source_authority_context"));
    }
    let event = event_json()?;
    let repository_id = positive_u64("GITHUB_REPOSITORY_ID")?;
    let current_run = positive_u64("GITHUB_RUN_ID")?;
    let current_attempt = u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let (source_sha, ci_run_id, ci_attempt, authority) =
        source_from_invocation(&event, repository_id, &seed)?;
    if !valid_git_sha(&source_sha) || !valid_git_sha(&authority) {
        return Err(internal("preview_source_sha"));
    }

    let api = GitHubApi::new(repository);
    api.verify_repository_identity(repository_id)?;
    api.verify_preview_main_ancestry(
        repository_id,
        &seed.default_branch,
        &source_sha,
        &coordinator,
    )?;
    let ci_source = RunSource {
        repository_id,
        sha: &source_sha,
        branch: &seed.default_branch,
    };
    api.verify_ci_identity(ci_run_id, ci_attempt, ci_source, true)?;

    let workflow =
        api.admitted_workflow(seed.channel.workflow_path(), seed.channel.workflow_name())?;
    let current_source = RunSource {
        repository_id,
        sha: &coordinator,
        branch: &seed.default_branch,
    };
    let role = release_job_name_seed(seed.channel, &required("GITHUB_JOB")?)?;
    api.verify_current_role(
        &CurrentRun {
            id: current_run,
            attempt: current_attempt,
            source: current_source,
            workflow: &workflow,
            event: &required("GITHUB_EVENT_NAME")?,
        },
        &role,
    )?;
    let checkout_lfs = source_checkout_lfs(&api, &source_sha)?;
    deadline
        .remaining()
        .map_err(|_| internal("preview_source_deadline"))?;
    append_output(&format!(
        "source_sha={source_sha}\nworkflow_authority_sha={authority}\nci_run_id={ci_run_id}\nci_attempt={ci_attempt}\ncheckout_lfs={checkout_lfs}\n"
    ))
}

fn source_checkout_lfs(api: &GitHubApi, source_sha: &str) -> Result<bool, OrchestratorError> {
    let mut files = SourceFiles::new(api, source_sha)?;
    let regular_files = files.regular_files()?;
    let Some(config) = regular_files
        .iter()
        .find(|entry| entry.path == ".velnor/config.toml")
    else {
        return Ok(false);
    };
    let _ = config;
    let bytes = files.read(".velnor/config.toml")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("preview_source_config_utf8"))?;
    config_checkout_lfs(Some(text))
}

fn config_checkout_lfs(text: Option<&str>) -> Result<bool, OrchestratorError> {
    let Some(text) = text else {
        return Ok(false);
    };
    Ok(crate::config::load_config_from_text(text)?
        .workflow
        .checkout_lfs)
}

fn source_from_invocation(
    event: &Value,
    repository_id: u64,
    seed: &PackageReleasePolicy,
) -> Result<(String, u64, u32, String), OrchestratorError> {
    match required("GITHUB_EVENT_NAME")?.as_str() {
        "workflow_run" => {
            let (source, run, attempt) =
                source_from_workflow_run_event(event, repository_id, &seed.repository)?;
            Ok((source, run, attempt, required("GITHUB_SHA")?))
        }
        "workflow_dispatch" => {
            let signed = super::super::recovery::verify_preview_original_for_source_selection(
                seed,
                super::super::recovery::PreviewRecoveryVerificationStage::SourceSelection,
            )?;
            let receipt = signed.receipt();
            let source_sha = receipt.source.commit_sha.clone();
            let api = GitHubApi::new(seed.repository.clone());
            let workflow = api.admitted_workflow(
                velnor_actions_contract::CI_WORKFLOW_PATH,
                velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            )?;
            let latest = api.latest_source_ci(
                &workflow,
                RunSource {
                    repository_id,
                    sha: &source_sha,
                    branch: &seed.default_branch,
                },
            )?;
            Ok((
                source_sha,
                latest
                    .id
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
                latest
                    .run_attempt
                    .filter(|attempt| *attempt > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
                required("GITHUB_SHA")?,
            ))
        }
        _ => Err(internal("preview_source_event_name")),
    }
}

fn source_from_workflow_run_event(
    event: &Value,
    repository_id: u64,
    repository: &str,
) -> Result<(String, u64, u32), OrchestratorError> {
    let run = &event["workflow_run"];
    if event["action"].as_str() != Some("completed")
        || event["repository"]["id"].as_u64() != Some(repository_id)
        || event["repository"]["full_name"].as_str() != Some(repository)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
        || run["head_branch"].as_str() != Some("main")
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("completed")
        || run["conclusion"].as_str() != Some("success")
        || run["path"].as_str() != Some(velnor_actions_contract::CI_WORKFLOW_PATH)
        || run["name"].as_str() != Some(velnor_actions_contract::WORKFLOW_DISPLAY_NAME)
    {
        return Err(internal("preview_source_event_identity"));
    }
    Ok((
        required_value(&run["head_sha"], "preview_source_event_identity")?,
        positive_value(&run["id"], "preview_source_event_identity")?,
        u32::try_from(positive_value(
            &run["run_attempt"],
            "preview_source_event_identity",
        )?)
        .map_err(|_| internal("preview_source_event_identity"))?,
    ))
}

fn parse_seed_policy() -> Result<PackageReleasePolicy, OrchestratorError> {
    let policy_text = required("VELNOR_RELEASE_POLICY")?;
    let expected_path = runner_root()?.join(SOURCE_SEED_POLICY_FILE);
    if Path::new(&required("VELNOR_REQUEST_FILE")?) != expected_path {
        return Err(internal("preview_source_policy_seed_path"));
    }
    let bytes =
        crate::retrieve_reports::staged_reads::read_staged_bytes(&expected_path, 1024 * 1024)
            .map_err(|_| internal("preview_source_policy_seed_read"))?;
    let policy = velnor_actions_contract::parse_package_release_policy(policy_text.as_bytes())
        .map_err(|_| internal("preview_source_policy_seed"))?;
    let canonical = policy
        .canonical_bytes()
        .map_err(|_| internal("preview_source_policy_seed"))?;
    if bytes != canonical || policy_text.as_bytes() != canonical.as_slice() {
        return Err(internal("preview_source_policy_seed_mismatch"));
    }
    Ok(policy)
}

fn event_json() -> Result<Value, OrchestratorError> {
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes(
        Path::new(&required("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
    )
    .map_err(|_| internal("preview_source_event_read"))?;
    velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
        .map_err(|_| internal("preview_source_event_json"))
}

fn verify_checkout(
    workspace: &Path,
    source_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let head = velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()])
        .run_in_until(workspace, deadline)
        .map_err(|_| internal("preview_source_checkout_head"))?;
    if !head.success
        || head
            .stdout_text("git")
            .map_err(|_| internal("preview_source_checkout_head"))?
            .trim()
            != source_sha
    {
        return Err(internal("preview_source_checkout_head"));
    }
    let shallow =
        velnor_actions_mise::GitRequest::rev_parse(vec!["--is-shallow-repository".into()])
            .run_in_until(workspace, deadline)
            .map_err(|_| internal("preview_source_checkout_history"))?;
    if !shallow.success
        || shallow
            .stdout_text("git")
            .map_err(|_| internal("preview_source_checkout_history"))?
            .trim()
            != "false"
    {
        return Err(internal("preview_source_checkout_history"));
    }
    Ok(())
}

fn read_local(root: &Path, path: &str) -> Result<Vec<u8>, OrchestratorError> {
    let read = crate::safe_read::read_repo_file(root, path, 1024 * 1024)?;
    match read {
        crate::safe_read::RepoRead::Text(text) => Ok(text.into_bytes()),
        crate::safe_read::RepoRead::Absent => Err(internal("preview_source_file_missing")),
    }
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn output_value(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn positive_value(value: &Value, reason: &'static str) -> Result<u64, OrchestratorError> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| internal(reason))
}

fn required_value(value: &Value, reason: &'static str) -> Result<String, OrchestratorError> {
    value
        .as_str()
        .filter(|value| valid_git_sha(value))
        .map(str::to_owned)
        .ok_or_else(|| internal(reason))
}

fn release_job_name_seed(
    channel: PackageReleaseChannel,
    job: &str,
) -> Result<String, OrchestratorError> {
    if channel == PackageReleaseChannel::Preview && job == "preview-eligibility" {
        return Ok("Select authenticated main source for preview".to_owned());
    }
    Err(internal("preview_source_role"))
}

fn _file_mode_is_regular(mode: &str) -> bool {
    matches!(mode, "100644" | "100755")
}

#[cfg(test)]
#[path = "preview_source_admission_tests.rs"]
mod tests;
