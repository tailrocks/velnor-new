//! Schema-2 YAML rendering for cancellation probe jobs.

use std::collections::BTreeMap;

use super::{MbxQualificationPins, Phase};
use crate::render::RenderContext;
use crate::yaml::Yaml;
use crate::{RenderError, document};
use velnor_actions_contract::{Job, PermissionLevel, PullRequestCachePolicy, Step};

const VICTIM_RECEIPT_UPLOAD: &str = "Upload MBX cancellation receipt";

pub(super) fn render_victim_job(
    job: Job,
    id: &str,
    hosted: &Yaml,
    phase: Phase,
    request: &MbxQualificationPins,
) -> Result<(String, Yaml), RenderError> {
    validate_render_job(&job, hosted, true)?;
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    let condition = job
        .condition
        .clone()
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cancel_job_gate_missing".to_owned()))?;
    let steps = render_victim_steps(&job, id, phase, request, runs_on)?;
    Ok(render_job(job, id, runs_on, &condition, steps))
}

fn render_victim_steps(
    job: &Job,
    id: &str,
    phase: Phase,
    request: &MbxQualificationPins,
    runs_on: &str,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered = vec![super::probe_steps::victim_identity_yaml(request, phase)];
    let mut upload_count = 0;
    for step in &job.steps {
        if step.name == VICTIM_RECEIPT_UPLOAD {
            upload_count += 1;
            if upload_count != 1 {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_cancel_duplicate_victim_receipt_upload".to_owned(),
                ));
            }
            if phase == Phase::DuringSave {
                rendered.push(super::probe_steps::source_fetch_yaml(None));
                rendered.push(super::probe_steps::workspace_build_yaml(request, None));
            }
            rendered.push(super::probe_steps::victim_receipt_yaml(request, phase));
            rendered.push(typed_step(id, step, runs_on)?);
            if phase == Phase::PreSave {
                rendered.push(super::probe_steps::no_write_guard_yaml());
                rendered.push(super::probe_steps::wait_before_save_yaml());
            }
        } else {
            rendered.push(typed_step(id, step, runs_on)?);
        }
    }
    if upload_count != 1 {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_victim_receipt_upload_missing".to_owned(),
        ));
    }
    Ok(rendered)
}

fn validate_render_job(job: &Job, hosted: &Yaml, victim: bool) -> Result<(), RenderError> {
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    if &job.runs_on != runs_on {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_runner_mismatch".to_owned(),
        ));
    }
    if job.condition.is_none() {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_job_gate_missing".to_owned(),
        ));
    }
    if victim && job.permissions.is_none() {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_victim_permissions_missing".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn typed_step(id: &str, step: &Step, runs_on: &str) -> Result<Yaml, RenderError> {
    document::step_to_yaml(id, step, &step_context(runs_on), &[], false)
}

pub(super) fn bash_step(
    name: &str,
    id: Option<&str>,
    script: &str,
    env: &BTreeMap<String, String>,
) -> Yaml {
    bash_step_inner(name, id, script, env, false, None)
}

pub(super) fn bash_step_if(
    name: &str,
    script: &str,
    env: &BTreeMap<String, String>,
    condition: Option<&str>,
) -> Yaml {
    bash_step_inner(name, None, script, env, false, condition)
}

pub(super) fn token_bash_step(
    name: &str,
    id: Option<&str>,
    script: &str,
    env: &BTreeMap<String, String>,
    condition: Option<&str>,
) -> Result<Yaml, RenderError> {
    if env.contains_key("GH_TOKEN") {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_gh_token_override".to_owned(),
        ));
    }
    Ok(bash_step_inner(name, id, script, env, true, condition))
}

pub(super) fn render_raw_job(
    job: Job,
    id: &str,
    hosted: &Yaml,
    rendered_steps: Vec<Yaml>,
) -> Result<(String, Yaml), RenderError> {
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    if &job.runs_on != runs_on {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_runner_mismatch".to_owned(),
        ));
    }
    let condition = job
        .condition
        .clone()
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_cancel_job_gate_missing".to_owned()))?;
    Ok(render_job(job, id, runs_on, &condition, rendered_steps))
}

fn render_job(
    job: Job,
    id: &str,
    runs_on: &str,
    condition: &str,
    rendered_steps: Vec<Yaml>,
) -> (String, Yaml) {
    let title = job.display_name.clone();
    let timeout = i64::from(job.timeout_minutes.minutes());
    let mut fields = super::super::features::base(&title, Yaml::str(runs_on), timeout);
    if !job.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(job.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(permissions) = job.permissions {
        fields.push((
            "permissions".to_owned(),
            permission_yaml(
                permissions.actions,
                permissions.contents,
                permissions.pull_requests,
                permissions.id_token,
            ),
        ));
    }
    if let Some(environment) = job.environment {
        fields.push(("environment".to_owned(), Yaml::str(environment)));
    }
    let rendered = super::super::features::finish(id, fields, rendered_steps);
    super::super::features::gated(rendered, condition)
}

fn bash_step_inner(
    name: &str,
    id: Option<&str>,
    script: &str,
    env: &BTreeMap<String, String>,
    token: bool,
    condition: Option<&str>,
) -> Yaml {
    let mut entries = vec![("name".to_owned(), Yaml::str(name))];
    if let Some(id) = id {
        entries.push(("id".to_owned(), Yaml::str(id)));
    }
    entries.push(("shell".to_owned(), Yaml::str("bash")));
    if token || !env.is_empty() {
        let mut values: Vec<(String, Yaml)> = env
            .iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect();
        if token {
            values.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
        }
        entries.push(("env".to_owned(), Yaml::Map(values)));
    }
    if let Some(condition) = condition {
        entries.push(("if".to_owned(), Yaml::str(condition)));
    }
    let body = format!(
        "{}\n{}\n{script}",
        crate::schema2::mbx_stock_restore::STOCK_RESTORE_CLASSIFIER_SCRIPT,
        super::private_io::PRIVATE_IO_HELPERS
    );
    entries.push(("run".to_owned(), Yaml::str(body)));
    Yaml::Map(entries)
}

fn permission_yaml(
    actions: PermissionLevel,
    contents: PermissionLevel,
    pull_requests: PermissionLevel,
    id_token: PermissionLevel,
) -> Yaml {
    Yaml::Map(vec![
        ("actions".to_owned(), Yaml::str(permission(actions))),
        ("contents".to_owned(), Yaml::str(permission(contents))),
        (
            "pull-requests".to_owned(),
            Yaml::str(permission(pull_requests)),
        ),
        ("id-token".to_owned(), Yaml::str(permission(id_token))),
    ])
}

fn permission(value: PermissionLevel) -> &'static str {
    match value {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

fn step_context(runs_on: &str) -> RenderContext {
    RenderContext {
        generator_version: "0.0.0".to_owned(),
        runs_on: runs_on.to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.0.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor".to_owned(),
        checkout_uses: super::super::features::CHECKOUT_USES.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}
