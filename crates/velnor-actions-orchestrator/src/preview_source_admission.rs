// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub const FINALIZE_PREVIEW_SOURCE_OP: &str = "finalize-preview-source-v1";

pub fn select_preview_source_from_env() -> Result<(), OrchestratorError> {
    select()
}

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
#[path = "preview_source_admission_selection.rs"]
mod selection;

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
    let workflow_sha = required("GITHUB_WORKFLOW_SHA")?;
    let ref_name = required("GITHUB_REF")?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    let event_name = required("GITHUB_EVENT_NAME")?;
    selection::validate_preview_source_context(
        &seed,
        &repository,
        &coordinator,
        &workflow_sha,
        &ref_name,
        &workflow_ref,
        &event_name,
    )?;
    let event = event_json()?;
    let repository_id = positive_u64("GITHUB_REPOSITORY_ID")?;
    let current_run = positive_u64("GITHUB_RUN_ID")?;
    let current_attempt = u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let api = GitHubApi::new(repository);
    selection::select_and_emit_source(
        &api,
        &selection::SelectionRequest {
            event: &event,
            seed: &seed,
            repository_id,
            authority: &coordinator,
            current_run,
            current_attempt,
            event_name: &event_name,
            job_name: &required("GITHUB_JOB")?,
            deadline,
        },
        append_output,
    )
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
