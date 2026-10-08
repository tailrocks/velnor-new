//! Bounded exact-trust input for Linux connect.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;

use velnor_runner_host::JobTrustPolicy;

use super::format::toml_string;
use super::{ConnectError, ConnectRequest};

const MAX_TRUST_POLICY_INPUT_BYTES: usize = 64 * 1024;

pub(super) fn append_legacy_platform_trust(
    text: &mut String,
    request: &ConnectRequest<'_>,
) -> Result<(), ConnectError> {
    if request.allowed_events.is_empty() {
        return Ok(());
    }
    let repository = toml_string(request.repo)?;
    let events = string_array(request.allowed_events)?;
    let workflow_paths = string_array(request.allowed_workflow_paths)?;
    write!(
        text,
        "[trust]\nallowed_repositories = [{repository}]\nallowed_events = {events}\nallowed_workflow_paths = {workflow_paths}\nallow_forks = false\n"
    )
    .map_err(|_| ConnectError::Write)
}

pub(super) fn load_linux_trust_policy(
    request: &ConnectRequest<'_>,
) -> Result<JobTrustPolicy, ConnectError> {
    let Some(path) = request.trust_policy_file.as_deref() else {
        return Err(ConnectError::TrustPolicy);
    };
    let file = File::open(path).map_err(|_| ConnectError::TrustPolicy)?;
    let metadata = file.metadata().map_err(|_| ConnectError::TrustPolicy)?;
    if !metadata.is_file() || metadata.len() > MAX_TRUST_POLICY_INPUT_BYTES as u64 {
        return Err(ConnectError::TrustPolicy);
    }
    let mut bytes = Vec::new();
    file.take((MAX_TRUST_POLICY_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConnectError::TrustPolicy)?;
    if bytes.len() > MAX_TRUST_POLICY_INPUT_BYTES {
        return Err(ConnectError::TrustPolicy);
    }
    let policy: JobTrustPolicy =
        serde_json::from_slice(&bytes).map_err(|_| ConnectError::TrustPolicy)?;
    if policy.allowed_repositories.len() != 1
        || policy.allowed_repositories[0] != request.repo
        || policy.allowed_events != request.allowed_events
        || policy.allowed_workflow_paths != request.allowed_workflow_paths
        || policy.allowed_head_branches.is_empty()
        || policy.workflow_rules.is_empty()
        || policy.allowed_group_workflows.is_empty()
        || policy.allow_forks
    {
        return Err(ConnectError::TrustPolicy);
    }
    Ok(policy)
}

pub(super) fn append_trust_policy(
    text: &mut String,
    policy: Option<&JobTrustPolicy>,
) -> Result<(), ConnectError> {
    let Some(policy) = policy else {
        return Ok(());
    };
    writeln!(
        text,
        "[trust]\nallowed_repositories = {}\nallowed_events = {}\nallowed_workflow_paths = {}\nallowed_head_branches = {}\nallowed_group_workflows = {}\nallow_forks = {}",
        string_array(&policy.allowed_repositories)?,
        string_array(&policy.allowed_events)?,
        string_array(&policy.allowed_workflow_paths)?,
        string_array(&policy.allowed_head_branches)?,
        string_array(&policy.allowed_group_workflows)?,
        policy.allow_forks,
    )
    .map_err(|_| ConnectError::Write)?;
    for rule in &policy.workflow_rules {
        writeln!(
            text,
            "\n[[trust.workflow_rules]]\nworkflow_ref = {}\njob_workflow_ref = {}\nworkflow_path = {}\nevent = {}\nhead_branch = {}",
            toml_string(&rule.workflow_ref)?,
            toml_string(&rule.job_workflow_ref)?,
            toml_string(&rule.workflow_path)?,
            toml_string(&rule.event)?,
            toml_string(&rule.head_branch)?,
        )
        .map_err(|_| ConnectError::Write)?;
        for workflow in &rule.referenced_workflows {
            writeln!(
                text,
                "\n[[trust.workflow_rules.referenced_workflows]]\npath = {}\ngit_ref = {}\nsha = {}",
                toml_string(&workflow.path)?,
                toml_string(&workflow.git_ref)?,
                toml_string(&workflow.sha)?,
            )
            .map_err(|_| ConnectError::Write)?;
        }
    }
    Ok(())
}

fn string_array(values: &[String]) -> Result<String, ConnectError> {
    values
        .iter()
        .map(|value| toml_string(value))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| format!("[{}]", values.join(", ")))
}
