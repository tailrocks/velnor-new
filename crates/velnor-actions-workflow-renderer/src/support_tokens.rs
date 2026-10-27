// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::toolchain_env::STEP_PRIVATE_FETCH_ALLOWLIST;
use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
};

#[path = "support_private_tokens.rs"]
mod private;

pub(crate) fn check_token_hygiene(
    jobs: &BTreeMap<String, Job>,
    report_helper_version: &str,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for step in &job.steps {
            check_step_tokens(id, step, report_helper_version)?;
        }
    }
    Ok(())
}

fn is_ambient_auth_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreparePinnedTools
                | StepRole::PrepareRustComponents
                | StepRole::CargoSourcesFetch
                | StepRole::CargoDeny
                | StepRole::CargoMachete
                | StepRole::Jactionlint
                | StepRole::Markdownlint
                | StepRole::StrictJson
                | StepRole::FrontmatterId
                | StepRole::LinkCheck
                | StepRole::NativeValidators
        )
    )
}

fn check_step_tokens(
    id: &str,
    step: &Step,
    report_helper_version: &str,
) -> Result<(), RenderError> {
    private::check_private_binding(id, step)?;
    check_github_evidence_binding(id, step)?;
    if step.role == Some(StepRole::PrivateTokenMint) {
        return Ok(());
    }
    match &step.kind {
        StepKind::Shell { run, env } => {
            check_env_tokens(id, env, false)?;
            for arg in strip_unset_argv(run) {
                if names_token(&strip_unset_prelude(arg)) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_run:{id}:{}",
                        step.name
                    )));
                }
            }
            check_scrub_coverage(id, &step.name, step.role, env)?;
        }
        StepKind::TaskExecution { argv, env, .. } => {
            check_env_tokens(id, env, false)?;
            for arg in argv {
                if names_token(arg) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_run:{id}:{}",
                        step.name
                    )));
                }
            }
        }
        StepKind::Action { with, env, .. } => {
            for value in with.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_input:{id}:{}",
                        step.name
                    )));
                }
            }
            for value in env.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_env:{id}:{}",
                        step.name
                    )));
                }
            }
        }
        StepKind::Internal { env, .. } => {
            if private::is_private_binding_role(step.role)
                || step.role == Some(StepRole::GitHubEvidenceFetch)
            {
                check_env_tokens(id, env, step.role == Some(StepRole::GitHubEvidenceFetch))?;
            }
        }
        StepKind::NativeCheckExecution {
            check_id,
            task_id,
            sdk_version,
            ..
        } => validate_native_check_token_scope(
            id,
            check_id,
            task_id,
            sdk_version,
            report_helper_version,
        )?,
    }
    if matches!(&step.kind, StepKind::Internal { operation, .. }
        if operation == crate::steps::RESOLVE_QUALIFICATION_OPERATION)
        && (id != PLAN_JOB_ID
            || step.condition.as_deref() != Some("github.event_name == 'workflow_dispatch'"))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "qualification_resolver_scope:{id}:{}",
            step.name
        )));
    }
    Ok(())
}

fn is_scoped_gh_token(id: &str, env: &BTreeMap<String, String>) -> bool {
    (id == PLAN_JOB_ID || id == FINAL_JOB_ID)
        && env
            .get("GH_TOKEN")
            .is_some_and(|value| value == "${{ github.token }}")
}

fn validate_native_check_token_scope(
    job_id: &str,
    check_id: &str,
    task_id: &str,
    sdk_version: &str,
    report_helper_version: &str,
) -> Result<(), RenderError> {
    if !check_id.is_empty()
        && job_id == format!("check-{check_id}")
        && task_id == format!("internal/native-check/{check_id}")
        && sdk_version == report_helper_version
    {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "native_check_token_scope:{job_id}"
        )))
    }
}

fn is_scoped_evidence_token(
    id: &str,
    env: &BTreeMap<String, String>,
    evidence_fetch: bool,
) -> bool {
    evidence_fetch
        && id.starts_with("github-evidence-fetch-")
        && env
            .get("GH_TOKEN")
            .is_some_and(|value| value == "${{ github.token }}")
}

fn check_env_tokens(
    id: &str,
    env: &BTreeMap<String, String>,
    evidence_fetch: bool,
) -> Result<(), RenderError> {
    use crate::toolchain_env::is_denied_credential_key;
    for (key, value) in env {
        if STEP_PRIVATE_FETCH_ALLOWLIST.contains(&key.as_str()) {
            continue;
        }
        if is_denied_credential_key(key) {
            if value.is_empty() {
                continue;
            }
            if key == "GH_TOKEN" {
                if is_scoped_gh_token(id, env) || is_scoped_evidence_token(id, env, evidence_fetch)
                {
                    continue;
                }
                return Err(RenderError::InvalidWorkflow(format!(
                    "token_misplaced:{id}:GH_TOKEN"
                )));
            }
            return Err(RenderError::InvalidWorkflow(format!(
                "credential_env:{id}:{key}"
            )));
        }
        if names_token(value) {
            return Err(RenderError::InvalidWorkflow(format!(
                "token_in_env:{id}:{key}"
            )));
        }
    }
    Ok(())
}

fn check_github_evidence_binding(id: &str, step: &Step) -> Result<(), RenderError> {
    let fail = || {
        RenderError::InvalidWorkflow(format!("github_evidence_binding_shape:{id}:{}", step.name))
    };
    let StepKind::Internal { operation, env } = &step.kind else {
        if matches!(
            step.role,
            Some(StepRole::GitHubEvidenceFetch | StepRole::GitHubEvidenceImport)
        ) {
            return Err(fail());
        }
        return Ok(());
    };
    let evidence_operation = operation == velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION
        || operation == velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION;
    if !evidence_operation
        && matches!(
            step.role,
            Some(StepRole::GitHubEvidenceFetch | StepRole::GitHubEvidenceImport)
        )
    {
        return Err(fail());
    }
    if !evidence_operation {
        return Ok(());
    }
    let spec = env.get(velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV);
    if spec.is_none_or(String::is_empty)
        || step.parallel
        || step.condition.is_some()
        || step.id.is_some()
    {
        return Err(fail());
    }
    let Some(collector_suite_id) = spec.and_then(|value| valid_evidence_spec_suite_id(value))
    else {
        return Err(fail());
    };
    match operation.as_str() {
        velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION
            if step.role == Some(StepRole::GitHubEvidenceFetch)
                && id == format!("github-evidence-fetch-{collector_suite_id}")
                && env.len() == 2
                && env.get("GH_TOKEN").map(String::as_str) == Some("${{ github.token }}") =>
        {
            Ok(())
        }
        velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION
            if step.role == Some(StepRole::GitHubEvidenceImport)
                && id == format!("mise-task-{collector_suite_id}")
                && env.len() == 1
                && !env.contains_key("GH_TOKEN") =>
        {
            Ok(())
        }
        _ => Err(fail()),
    }
}

fn valid_evidence_spec_suite_id(value: &str) -> Option<String> {
    let spec =
        velnor_actions_contract::GitHubEvidenceFetchSpecV1::parse_canonical_json(value).ok()?;
    Some(spec.collector_suite_id)
}

fn check_scrub_coverage(
    id: &str,
    name: &str,
    role: Option<StepRole>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    use crate::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    if is_ambient_auth_role(role) {
        return Ok(());
    }
    let scoped = is_scoped_gh_token(id, env);
    let scrubbed = STEP_CREDENTIAL_DENYLIST
        .iter()
        .all(|key| env.get(*key).is_some_and(String::is_empty) || (*key == "GH_TOKEN" && scoped));
    if scrubbed {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "missing_scrub:{id}:{name}"
        )))
    }
}

fn strip_unset_argv(run: &[String]) -> &[String] {
    &run[crate::toolchain_env::unset_prefix_len(run)..]
}

fn strip_unset_prelude(script: &str) -> String {
    use crate::toolchain_env::credential_unset_prelude;
    let prelude = credential_unset_prelude();
    for (at, _) in script.match_indices(prelude.as_str()) {
        if is_prelude_command_unit(&script[..at], &script[at + prelude.len()..]) {
            let mut stripped = String::with_capacity(script.len() - prelude.len());
            stripped.push_str(&script[..at]);
            stripped.push_str(&script[at + prelude.len()..]);
            return stripped;
        }
    }
    script.to_owned()
}

fn is_prelude_command_unit(before: &str, after: &str) -> bool {
    const LEFT: [&str; 6] = ["&& ", "; ", "{ ", "\n", "( ", "("];
    const RIGHT: [char; 8] = [' ', ';', '&', '\n', '|', ')', '}', '\t'];
    (before.is_empty() || LEFT.iter().any(|sep| before.ends_with(sep)))
        && (after.is_empty() || after.starts_with(RIGHT))
}

fn names_token(text: &str) -> bool {
    text.contains("_TOKEN")
        || text.contains("ACTIONS_ID_TOKEN_REQUEST_URL")
        || text.contains("CARGO_REGISTRIES_")
        || text.contains("github.token")
        || text.to_ascii_lowercase().contains("secrets.")
}
