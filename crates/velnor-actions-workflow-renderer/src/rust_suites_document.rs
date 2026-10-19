//! Standalone YAML workflow document builder for declared Rust test suites.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, PermissionLevel, RunsOn, RustSuiteConfig, Step, StepKind, StepRole,
};

use super::{RustSuitesRenderSpec, build_suite_jobs_for_entry};

#[path = "rust_suites_document_inputs.rs"]
mod inputs;
use crate::{
    RenderError,
    document_steps::{non_internal_step_to_yaml, string_map_yaml},
    matrix,
    render::fallback::render_checked_workflow,
    steps,
    workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml},
    yaml::Yaml,
};
use inputs::{suite_job_condition, triggers_yaml, workflow_display_name};

pub(super) fn render_workflow_for_slug(
    slug: &str,
    entries: &[&RustSuiteConfig],
    spec: &RustSuitesRenderSpec,
    routing: &WorkflowRoutingPolicy,
    helper_steps: &BTreeMap<String, Step>,
) -> Result<String, RenderError> {
    let mut jobs = BTreeMap::new();
    let mut allow_failure_jobs = BTreeSet::new();
    for suite in entries {
        let condition = suite_job_condition(suite);
        if suite.allow_failure {
            allow_failure_jobs.insert(format!("suite-{}", suite.id));
        }
        for (job_id, mut job) in build_suite_jobs_for_entry(suite, spec, true)? {
            job.condition.clone_from(&condition);
            jobs.insert(job_id, job);
        }
    }
    finalize_suite_workflow_jobs(&mut jobs, spec, helper_steps)?;
    let suite_matrices = matrix::suites::suite_matrices_of(&jobs)?;
    let scrubbed = if suite_matrices.is_empty() {
        jobs
    } else {
        matrix::scrub_matrix_marker(&jobs)
    };
    let mbx_jobs = crate::mbx_gc_policy::jobs_with_mbx_objects(&scrubbed);
    let workflow_env = crate::toolchain_env::workflow_level_env();
    let mut rendered_jobs = Vec::with_capacity(scrubbed.len());
    for (id, job) in &scrubbed {
        rendered_jobs.push((
            id.clone(),
            suite_job_to_yaml(
                id,
                job,
                &workflow_env,
                mbx_jobs.contains(id),
                allow_failure_jobs.contains(id),
            )?,
        ));
    }
    let cancel_in_progress = entries
        .iter()
        .find_map(|suite| suite.cancel_in_progress)
        .unwrap_or(false);
    let cancel_yaml =
        if cancel_in_progress && entries.iter().any(|suite| suite.has_schedule_trigger()) {
            Yaml::str("${{ github.event_name != 'schedule' }}")
        } else {
            Yaml::Bool(cancel_in_progress)
        };
    let top = vec![
        ("name".to_owned(), Yaml::str(workflow_display_name(slug))),
        (
            "on".to_owned(),
            triggers_yaml(entries, spec.workflow_dispatch.as_ref()),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("env".to_owned(), string_map_yaml(&workflow_env)),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(format!(
                        "{slug}-${{{{ github.workflow }}}}-${{{{ github.ref }}}}-${{{{ github.event_name }}}}"
                    )),
                ),
                ("cancel-in-progress".to_owned(), cancel_yaml),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(rendered_jobs)),
    ];
    let mut document = Yaml::Map(top);
    matrix::suites::attach_suite_matrices(&mut document, &suite_matrices)?;
    let path = format!(".github/workflows/{slug}.yml");
    let document = route_workflow_yaml(&path, document, routing)?;
    let text = render_checked_workflow(&path, &document, &spec.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

fn finalize_suite_workflow_jobs(
    jobs: &mut BTreeMap<String, Job>,
    spec: &RustSuitesRenderSpec,
    helper_steps: &BTreeMap<String, Step>,
) -> Result<(), RenderError> {
    let mut used_helpers = BTreeSet::new();
    let default_target = crate::runs_on::target_for_runner(&spec.default_runs_on);
    for (id, job) in jobs.iter_mut() {
        let target = crate::runs_on::target_for_runner(&job.runs_on).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("tools_cache_unsupported_target:{id}"))
        })?;
        let setup = if default_target == Some(target) {
            spec.mise.clone()
        } else {
            spec.mise.for_target(target)?
        };
        if let Some(helper) = helper_steps.get(&job.runs_on) {
            insert_helper_after_checkout(id, job, helper)?;
            used_helpers.insert(job.runs_on.clone());
            crate::cache_p08::ensure_tools_cache_v2(
                id,
                job,
                &setup,
                true,
                target,
                &spec.checkout_uses,
            )?;
        } else {
            insert_setup_after_checkout(id, job, &setup)?;
        }
        crate::cache_p08::check_no_legacy_rust_cache(id, job)?;
        crate::cache_p08::check_mbx_before_fetch(id, job)?;
    }
    if helper_steps
        .keys()
        .any(|label| !used_helpers.contains(label))
    {
        return Err(RenderError::InvalidWorkflow(
            "suite_helper_without_matching_runner".to_owned(),
        ));
    }
    crate::cache_p08::elect_cache_writers_with_policy(jobs, spec.pull_request_cache_policy)?;
    crate::cache_tar_setup::insert_launcher_steps(jobs)?;
    crate::cache_steps::append_workspace_cleanups(jobs)?;
    crate::cache_doctor::check(jobs)
}

fn insert_helper_after_checkout(
    job_id: &str,
    job: &mut Job,
    helper: &Step,
) -> Result<(), RenderError> {
    if helper.role != Some(StepRole::AcquireVelnor) {
        return Err(RenderError::InvalidWorkflow(format!(
            "suite_helper_role_mismatch:{job_id}"
        )));
    }
    let Some(checkout_at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::Checkout))
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "suite_checkout_missing:{job_id}"
        )));
    };
    job.steps.insert(checkout_at + 1, helper.clone());
    crate::closure::check_internal_staged(job_id, job, false)
}

fn insert_setup_after_checkout(
    job_id: &str,
    job: &mut Job,
    setup: &crate::MiseSetup,
) -> Result<(), RenderError> {
    let step = crate::cache_p08::setup_step(setup)?;
    let Some(checkout_at) = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::Checkout))
    else {
        return Err(RenderError::InvalidWorkflow(format!(
            "suite_checkout_missing:{job_id}"
        )));
    };
    job.steps.insert(checkout_at + 1, step);
    Ok(())
}

fn suite_job_to_yaml(
    id: &str,
    job: &Job,
    workflow_env: &BTreeMap<String, String>,
    native_mbx: bool,
    allow_failure: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(&runner, RunsOn::ScaleSet(_));
    let runs_on = match runner {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    };
    let job_env = extract_suite_job_env(job, native_mbx);
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if scale_set {
        entries.push(crate::runs_on::run_shell_defaults_field(
            crate::runs_on::SCALE_SET_RUN_SHELL,
        ));
    }
    let local_job_env = crate::document_env::exclude_inherited(&job_env, workflow_env);
    if !local_job_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&local_job_env)));
    }
    if let Some(permissions) = &job.permissions {
        let level = match permissions.contents {
            PermissionLevel::None => "none",
            PermissionLevel::Read => "read",
            PermissionLevel::Write => "write",
        };
        entries.push((
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str(level))]),
        ));
    }
    if !job.needs.is_empty() {
        entries.push((
            "needs".to_owned(),
            Yaml::Seq(job.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if allow_failure {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    let mut effective_env = workflow_env.clone();
    effective_env.extend(job_env);
    let mut rendered_steps = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered_steps.push(non_internal_step_to_yaml(
            id,
            step,
            &effective_env,
            &job.runs_on,
        )?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}

fn extract_suite_job_env(job: &Job, native_mbx: bool) -> BTreeMap<String, String> {
    let mut job_env = BTreeMap::new();
    for step in &job.steps {
        if let StepKind::Shell { env, .. } | StepKind::Action { env, .. } = &step.kind
            && let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN")
        {
            job_env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
        }
    }
    if native_mbx {
        use crate::cache_steps::{
            MBX_BUILD_SCRIPT_EXECUTION_ENV, MBX_BUILD_SCRIPT_EXECUTION_VALUE, MBX_CC_ENV,
            MBX_CC_VALUE, MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE, MBX_SHARE_OUT_DIR_ENV,
            MBX_SHARE_OUT_DIR_VALUE,
        };
        for (key, val) in [
            (MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE),
            (MBX_SHARE_OUT_DIR_ENV, MBX_SHARE_OUT_DIR_VALUE),
            (MBX_CC_ENV, MBX_CC_VALUE),
            (
                MBX_BUILD_SCRIPT_EXECUTION_ENV,
                MBX_BUILD_SCRIPT_EXECUTION_VALUE,
            ),
        ] {
            job_env.insert(key.to_owned(), val.to_owned());
        }
    }
    job_env
}
