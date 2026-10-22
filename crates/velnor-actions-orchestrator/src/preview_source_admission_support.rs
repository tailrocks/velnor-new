use super::super::{required, runner_root};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::path::Path;
use velnor_actions_contract::{PackageReleasePolicy, package_release::PackageReleaseChannel};
use velnor_actions_mise::CheckDeadline;

const SOURCE_SEED_POLICY_FILE: &str = "preview-source-seed-policy.json";

pub(super) fn parse_seed_policy() -> Result<PackageReleasePolicy, OrchestratorError> {
    let policy_text = required("VELNOR_RELEASE_POLICY")?;
    let expected_path = runner_root()?.join(SOURCE_SEED_POLICY_FILE);
    if required("VELNOR_REQUEST_FILE")? != expected_path {
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

pub(super) fn event_json() -> Result<Value, OrchestratorError> {
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes(
        Path::new(&required("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
    )
    .map_err(|_| internal("preview_source_event_read"))?;
    velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
        .map_err(|_| internal("preview_source_event_json"))
}

pub(super) fn verify_checkout(
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

pub(super) fn read_local(root: &Path, path: &str) -> Result<Vec<u8>, OrchestratorError> {
    let read = crate::safe_read::read_repo_file(root, path, 1024 * 1024)?;
    match read {
        crate::safe_read::RepoRead::Text(text) => Ok(text.into_bytes()),
        crate::safe_read::RepoRead::Absent => Err(internal("preview_source_file_missing")),
    }
}

pub(super) fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn output_value(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

pub(super) fn positive_value(
    value: &Value,
    reason: &'static str,
) -> Result<u64, OrchestratorError> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .ok_or_else(|| internal(reason))
}

pub(super) fn required_value(
    value: &Value,
    reason: &'static str,
) -> Result<String, OrchestratorError> {
    value
        .as_str()
        .filter(|value| valid_git_sha(value))
        .map(str::to_owned)
        .ok_or_else(|| internal(reason))
}

pub(super) fn release_job_name_seed(
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
