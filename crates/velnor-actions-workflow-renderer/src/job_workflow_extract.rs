//! Private lowering for typed source jobs into local reusable workflows.
//!
//! The caller keeps the original job identity, dependency list, and admission
//! condition. The callee keeps the same inner job id and exact body, with only
//! closed plan-input references substituted and parent-owned fields removed.

use std::collections::BTreeMap;

#[path = "job_workflow_extract_expression.rs"]
mod expression;
#[path = "job_workflow_extract_runner.rs"]
mod runner;
#[path = "job_workflow_extract_selection.rs"]
mod selection;
pub(crate) use expression::caller_condition_for_selection;
use expression::{rewrite_condition, rewrite_expressions};
#[cfg(any(test, feature = "test-render-capture"))]
pub(crate) use expression::{test_condition_has_unbound_needs, test_expression_has_unbound_needs};
#[cfg(any(test, feature = "test-render-capture"))]
pub(crate) use runner::{CaptureRunnerMetadata, capture_runner_metadata};
pub(crate) use selection::{JobWorkflowSelection, pair_is_relocatable};
use velnor_actions_contract::{CacheMode, HostContainerProfile, Job, Step, StepKind};

use crate::RenderError;

#[cfg(feature = "test-render-capture")]
#[path = "job_workflow_extract_capture.rs"]
mod capture;
#[cfg(all(test, feature = "test-render-capture"))]
pub(crate) use capture::validate_finalized_runner;
#[cfg(feature = "test-render-capture")]
pub(crate) use capture::{
    prepare_job_for_capture, rendered_child_has_unbound_needs, validate_capture_source_ids,
    validate_finalized_caller, validate_rendered_child_runner,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallerInput {
    pub(crate) name: &'static str,
    pub(crate) expression: &'static str,
}

pub(crate) const CALLER_INPUT_TYPE: &str = "string";
pub(crate) const CALLER_INPUT_REQUIRED: bool = true;
pub(crate) const PROTOTYPE_JOB_ID: &str = "rust-velnor-actions-cli__hosted";
pub(crate) const PROTOTYPE_LOCAL_JOB_ID: &str = "rust-velnor-actions-cli__local";

pub(crate) const CALLER_INPUTS: [CallerInput; 5] = [
    CallerInput {
        name: "plan_result",
        expression: "${{ needs.plan.result }}",
    },
    CallerInput {
        name: "qualification_campaign",
        expression: "${{ needs.plan.outputs.qualification_campaign }}",
    },
    CallerInput {
        name: "run_key_attempt",
        expression: "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}",
    },
    CallerInput {
        name: "sources_key",
        expression: "${{ needs.plan.outputs.sources_key }}",
    },
    CallerInput {
        name: "sources_restore_keys",
        expression: "${{ needs.plan.outputs.sources_restore_keys }}",
    },
];

const STEP_REFERENCE_BINDINGS: [(&str, &str); 4] = [
    ("needs.plan.result", "inputs.plan_result"),
    (
        "needs.plan.outputs.qualification_campaign",
        "inputs.qualification_campaign",
    ),
    ("needs.plan.outputs.sources_key", "inputs.sources_key"),
    (
        "needs.plan.outputs.sources_restore_keys",
        "inputs.sources_restore_keys",
    ),
];

#[derive(Clone, Debug)]
pub(crate) struct PreparedJob {
    pub(crate) source_id: String,
    pub(crate) callee_path: String,
    pub(crate) source_job: Job,
    pub(crate) child_job: Job,
    pub(crate) selection: JobWorkflowSelection,
}

/// Trusted source snapshot and its exact rendered reusable-workflow outputs.
#[derive(Clone, Debug)]
pub(crate) struct JobWorkflowRelocation {
    pub(crate) prepared: PreparedJob,
    pub(crate) expected_caller: crate::yaml::Yaml,
    pub(crate) expected_callee_file: crate::tree::RenderedFile,
}

/// Copy one finalized job into a child body and a caller-owned header snapshot.
///
/// The condition is intentionally absent from the child: GitHub evaluates the
/// original admission predicate on the reusable-workflow call in the parent.
pub(crate) fn prepare_job(source_id: &str, source: &Job) -> Result<PreparedJob, RenderError> {
    prepare_job_inner(source_id, source, JobWorkflowSelection::legacy_prototype())
}

pub(super) fn prepare_job_inner(
    source_id: &str,
    source: &Job,
    selection: JobWorkflowSelection,
) -> Result<PreparedJob, RenderError> {
    let allowlisted_ids = selection.ids();
    if selection.is_legacy_prototype() && source_id != PROTOTYPE_JOB_ID {
        return Err(invalid(source_id, "outside_single_job_prototype"));
    }
    if allowlisted_ids.is_some_and(|ids| !ids.contains(source_id)) {
        return Err(invalid(source_id, "source_id_not_in_typed_rust_allowlist"));
    }
    selection.validate_caller_condition(source_id, source.condition.as_deref())?;
    if source.needs.len() != 1 || source.needs.first().is_none_or(|need| need != "plan") {
        return Err(invalid(source_id, "source_dependencies_not_exactly_plan"));
    }
    if source.is_dependency_seed() {
        return Err(invalid(source_id, "dependency_seed_not_relocatable"));
    }
    validate_fixed_job_fields(source_id, source)?;
    let callee_path = format!(".github/workflows/ci-rust-{source_id}.yml");
    let _validated_path = crate::guard::validate_tree_path(&callee_path)?;
    let mut child_job = source.clone();
    child_job.needs.clear();
    child_job.condition = None;
    for (index, step) in child_job.steps.iter_mut().enumerate() {
        rewrite_step(source_id, index, step)?;
    }
    Ok(PreparedJob {
        source_id: source_id.to_owned(),
        callee_path,
        source_job: source.clone(),
        child_job,
        selection,
    })
}

impl PreparedJob {
    /// Re-derive the child from its trusted source snapshot before relocation.
    pub(crate) fn validate_source(&self) -> Result<(), RenderError> {
        let expected =
            prepare_job_inner(&self.source_id, &self.source_job, self.selection.clone())?;
        if expected.callee_path != self.callee_path
            || !jobs_equal(&expected.child_job, &self.child_job)
        {
            return Err(invalid(&self.source_id, "source_child_snapshot_mismatch"));
        }
        Ok(())
    }
}

fn jobs_equal(left: &Job, right: &Job) -> bool {
    left.display_name == right.display_name
        && left.runs_on == right.runs_on
        && left.check_runner == right.check_runner
        && left.timeout_minutes == right.timeout_minutes
        && left.needs == right.needs
        && left.condition == right.condition
        && left.permissions == right.permissions
        && left.cache_mode == right.cache_mode
        && left.environment == right.environment
        && left.services == right.services
        && left.steps == right.steps
}

fn validate_fixed_job_fields(job_id: &str, job: &Job) -> Result<(), RenderError> {
    for (field, value) in [
        ("display_name", job.display_name.as_str()),
        ("runs_on", job.runs_on.as_str()),
    ] {
        reject_rewrite(job_id, field, value)?;
    }
    if let Some(runner) = &job.check_runner {
        reject_rewrite(job_id, "check_runner.label", &runner.label)?;
        if let Some(container) = &runner.container {
            validate_container_profile(job_id, container)?;
        }
    }
    if let Some(CacheMode::Expression(value)) = &job.cache_mode {
        reject_rewrite(job_id, "cache_mode", value)?;
    }
    if let Some(environment) = &job.environment {
        reject_rewrite(job_id, "environment", environment)?;
    }
    for (index, service) in job.services.iter().enumerate() {
        let prefix = format!("services[{index}]");
        for (field, value) in [
            ("id", service.id.as_str()),
            ("image", service.image.as_str()),
            ("options", service.options.as_str()),
        ] {
            reject_rewrite(job_id, &format!("{prefix}.{field}"), value)?;
        }
        reject_values_rewrite(job_id, &format!("{prefix}.ports"), &service.ports)?;
        reject_map(job_id, &format!("{prefix}.env"), &service.env)?;
    }
    Ok(())
}

fn validate_container_profile(
    job_id: &str,
    profile: &HostContainerProfile,
) -> Result<(), RenderError> {
    let cli = profile.cli();
    let daemon = profile.daemon();
    for (field, value) in [
        ("check_runner.container.context", profile.context()),
        ("check_runner.container.socket_path", profile.socket_path()),
        ("check_runner.container.cli.path", cli.path.as_str()),
        ("check_runner.container.cli.sha256", cli.sha256.as_str()),
        ("check_runner.container.cli.version", cli.version.as_str()),
        ("check_runner.container.cli.build", cli.build.as_str()),
        (
            "check_runner.container.daemon.version",
            daemon.version.as_str(),
        ),
        (
            "check_runner.container.daemon.operating_system",
            daemon.operating_system.as_str(),
        ),
    ] {
        reject_rewrite(job_id, field, value)?;
    }
    if let HostContainerProfile::OrbStack { sdk, .. } = profile {
        for (field, value) in [
            ("sdk.app_bundle_path", sdk.app_bundle_path.as_str()),
            ("sdk.bundle_id", sdk.bundle_id.as_str()),
            ("sdk.team_id", sdk.team_id.as_str()),
            ("sdk.version", sdk.version.as_str()),
            ("sdk.build", sdk.build.as_str()),
            ("sdk.info_plist_sha256", sdk.info_plist_sha256.as_str()),
            (
                "sdk.main_executable_path",
                sdk.main_executable_path.as_str(),
            ),
            (
                "sdk.main_executable_sha256",
                sdk.main_executable_sha256.as_str(),
            ),
            ("sdk.cli_bundle_path", sdk.cli_bundle_path.as_str()),
            ("sdk.source_tree_sha256", sdk.source_tree_sha256.as_str()),
            ("sdk.owned_tree_sha256", sdk.owned_tree_sha256.as_str()),
            ("sdk.cli_relative_path", sdk.cli_relative_path.as_str()),
            ("sdk.cli_sha256", sdk.cli_sha256.as_str()),
            ("sdk.cli_version", sdk.cli_version.as_str()),
            ("sdk.cli_build", sdk.cli_build.as_str()),
            ("sdk.cli_commit", sdk.cli_commit.as_str()),
            ("sdk.runtime_dir", sdk.runtime_dir.as_str()),
        ] {
            reject_rewrite(job_id, &format!("check_runner.container.{field}"), value)?;
        }
    }
    Ok(())
}

fn rewrite_step(job_id: &str, index: usize, step: &mut Step) -> Result<(), RenderError> {
    let prefix = format!("steps[{index}]");
    rewrite_value(job_id, &format!("{prefix}.name"), &mut step.name)?;
    if let Some(condition) = &mut step.condition {
        rewrite_condition(job_id, &format!("{prefix}.if"), condition)?;
    }
    match &mut step.kind {
        StepKind::Action { uses, with, env } => {
            reject_rewrite(job_id, &format!("{prefix}.uses"), uses)?;
            rewrite_map(job_id, &format!("{prefix}.with"), with)?;
            rewrite_map(job_id, &format!("{prefix}.env"), env)?;
        }
        StepKind::Shell { run, env } => {
            rewrite_values(job_id, &format!("{prefix}.run"), run)?;
            rewrite_map(job_id, &format!("{prefix}.env"), env)?;
        }
        StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest: _,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version,
            ..
        } => {
            reject_values_rewrite(job_id, &format!("{prefix}.argv"), argv)?;
            rewrite_map(job_id, &format!("{prefix}.env"), env)?;
            for (field, value) in [
                ("task_id", task_id.as_str()),
                ("matrix_id", matrix_id.as_str()),
                ("matrix_key", matrix_key.as_str()),
                ("report_helper_version", report_helper_version.as_str()),
                (
                    "toolchain.compile_driver",
                    toolchain_inputs.compile_driver.as_str(),
                ),
                (
                    "toolchain.test_runner",
                    toolchain_inputs.test_runner.as_str(),
                ),
            ] {
                reject_rewrite(job_id, &format!("{prefix}.{field}"), value)?;
            }
            reject_values_rewrite(
                job_id,
                &format!("{prefix}.toolchain.tools"),
                &toolchain_inputs.tools,
            )?;
            reject_values_rewrite(
                job_id,
                &format!("{prefix}.toolchain.components"),
                &toolchain_inputs.components,
            )?;
        }
        StepKind::NativeCheckExecution {
            check_id,
            task_id,
            sdk_version,
            ..
        } => {
            for (field, value) in [
                ("check_id", check_id.as_str()),
                ("task_id", task_id.as_str()),
                ("sdk_version", sdk_version.as_str()),
            ] {
                reject_rewrite(job_id, &format!("{prefix}.{field}"), value)?;
            }
        }
        StepKind::Internal { operation, env } => {
            reject_rewrite(job_id, &format!("{prefix}.operation"), operation)?;
            rewrite_map(job_id, &format!("{prefix}.env"), env)?;
        }
    }
    Ok(())
}

fn rewrite_values(job_id: &str, field: &str, values: &mut [String]) -> Result<(), RenderError> {
    for (index, value) in values.iter_mut().enumerate() {
        rewrite_value(job_id, &format!("{field}[{index}]"), value)?;
    }
    Ok(())
}

fn rewrite_map(
    job_id: &str,
    field: &str,
    values: &mut BTreeMap<String, String>,
) -> Result<(), RenderError> {
    for (key, value) in values {
        rewrite_value(job_id, &format!("{field}.{key}"), value)?;
    }
    Ok(())
}

fn reject_map(
    job_id: &str,
    field: &str,
    values: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    for (key, value) in values {
        reject_rewrite(job_id, &format!("{field}.{key}"), value)?;
    }
    Ok(())
}

fn reject_values_rewrite(job_id: &str, field: &str, values: &[String]) -> Result<(), RenderError> {
    for (index, value) in values.iter().enumerate() {
        reject_rewrite(job_id, &format!("{field}[{index}]"), value)?;
    }
    Ok(())
}

fn reject_rewrite(job_id: &str, field: &str, value: &str) -> Result<(), RenderError> {
    let rewritten = rewrite_expressions(value, job_id, field)?;
    if rewritten != value {
        return Err(invalid(
            job_id,
            &format!("plan_reference_in_fixed_field:{field}"),
        ));
    }
    Ok(())
}

fn rewrite_value(job_id: &str, field: &str, value: &mut String) -> Result<(), RenderError> {
    *value = rewrite_expressions(value, job_id, field)?;
    Ok(())
}

fn invalid(job_id: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("job_workflow_extract:{job_id}:{problem}"))
}

#[cfg(test)]
#[path = "job_workflow_extract_tests.rs"]
mod tests;
