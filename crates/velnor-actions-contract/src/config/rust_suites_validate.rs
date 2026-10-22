//! Fail-closed validation for `[[stacks.rust.suites]]`, `[workflow.dispatch]`, and `preserve_workflows`.

#[path = "rust_suites_validate_rules.rs"]
mod rules;

use std::collections::{BTreeMap, BTreeSet};

use crate::errors::ContractError;
use crate::workflow::ScheduleTrigger;

use super::rust_suites::{
    DEFAULT_SUITE_WORKFLOW_SLUG, GENERATOR_OWNED_WORKFLOW_FILES, MAX_SUITE_PARTITIONS,
    MAX_SUITE_TEST_THREADS, RESERVED_SUITE_IDS, RESERVED_SUITE_WORKFLOW_SLUGS, RustSuiteArtifact,
    RustSuiteConfig, RustSuiteTrigger, WorkflowDispatchConfig, WorkflowDispatchInputType,
};
use super::workflow::RUNNER_LABEL_CATALOG;
use rules::{
    is_valid_ascii_ident_with_hyphen, is_valid_dispatch_input_name, is_valid_workflow_slug,
    validate_dispatch_input, validate_suite_artifact, validate_suite_filters_and_dispatch,
    validate_suite_targets_and_env,
};

fn cfg_err(file: &str, key: impl Into<String>, problem: impl Into<String>) -> ContractError {
    ContractError::config(file, key, problem)
}

/// Validate a task-suite artifact using the shared suite path and retention policy.
/// Task suites have no shards, so artifact-name templates are not permitted.
pub(crate) fn validate_task_suite_artifact(
    artifact: &RustSuiteArtifact,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if artifact.name.contains('{') || artifact.name.contains('}') {
        return Err(cfg_err(
            file,
            format!("{prefix}.name"),
            format!("task_suite_artifact_name_must_be_literal:{}", artifact.name),
        ));
    }
    rules::validate_suite_artifact(artifact, file, prefix)
}

/// Validate all declared Rust suites (`stacks.rust.suites`).
///
/// # Errors
///
/// Returns [`ContractError::Config`] naming the offending suite field on any invalid input.
pub fn validate_rust_suites(suites: &[RustSuiteConfig], file: &str) -> Result<(), ContractError> {
    let mut seen_ids = BTreeSet::new();
    let mut cancel_by_slug: BTreeMap<&str, bool> = BTreeMap::new();
    for (index, suite) in suites.iter().enumerate() {
        let prefix = format!("stacks.rust.suites[{index}]");
        validate_suite_id(&suite.id, &mut seen_ids, file, &prefix)?;
        validate_suite_scalars(suite, file, &prefix)?;
        validate_suite_triggers_and_schedule(suite, file, &prefix)?;
        validate_suite_filters_and_dispatch(suite, file, &prefix)?;
        validate_suite_targets_and_env(suite, file, &prefix)?;
        if let Some(cancel) = suite.cancel_in_progress {
            let slug = suite
                .workflow
                .as_deref()
                .unwrap_or(DEFAULT_SUITE_WORKFLOW_SLUG);
            if let Some(&prev) = cancel_by_slug.get(slug) {
                if prev != cancel {
                    return Err(cfg_err(
                        file,
                        format!("{prefix}.cancel_in_progress"),
                        "conflicting_suite_cancel_in_progress",
                    ));
                }
            } else {
                cancel_by_slug.insert(slug, cancel);
            }
        }
        for (art_idx, artifact) in suite.artifacts.iter().enumerate() {
            validate_suite_artifact(artifact, file, &format!("{prefix}.artifacts[{art_idx}]"))?;
        }
    }
    Ok(())
}

/// Cross-validate `stacks.rust.suites` dispatch bindings against `workflow.dispatch`.
///
/// # Errors
///
/// Returns [`ContractError::Config`] when a suite references an undeclared dispatch
/// input or an option value not present in a `choice` input's `options`.
pub fn validate_suites_with_dispatch(
    suites: &[RustSuiteConfig],
    dispatch: &WorkflowDispatchConfig,
    file: &str,
) -> Result<(), ContractError> {
    for (index, suite) in suites.iter().enumerate() {
        let prefix = format!("stacks.rust.suites[{index}]");
        if let Some(filter_input) = &suite.filter_input
            && !dispatch.inputs.contains_key(filter_input)
        {
            return Err(cfg_err(
                file,
                format!("{prefix}.filter_input"),
                format!("unknown_dispatch_input:{filter_input}"),
            ));
        }
        for (input_name, values) in &suite.dispatch_inputs {
            let key_path = format!("{prefix}.dispatch_inputs.{input_name}");
            let Some(input_cfg) = dispatch.inputs.get(input_name) else {
                return Err(cfg_err(
                    file,
                    key_path,
                    format!("unknown_dispatch_input:{input_name}"),
                ));
            };
            if input_cfg.kind == WorkflowDispatchInputType::Choice {
                for value in values {
                    if !input_cfg.options.contains(value) {
                        return Err(cfg_err(
                            file,
                            key_path,
                            format!("unknown_dispatch_option:{value}"),
                        ));
                    }
                }
            }
        }
    }
    for name in dispatch.inputs.keys() {
        let used = suites.iter().any(|suite| {
            suite.has_workflow_dispatch_trigger()
                && (suite.filter_input.as_deref() == Some(name.as_str())
                    || suite.dispatch_inputs.contains_key(name.as_str()))
        });
        if !used {
            return Err(cfg_err(
                file,
                format!("workflow.dispatch.inputs.{name}"),
                format!("unused_dispatch_input:{name}"),
            ));
        }
    }
    Ok(())
}

impl WorkflowDispatchConfig {
    /// Validate `[workflow.dispatch]` inputs.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::Config`] on invalid input names, options, or defaults.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for (name, input) in &self.inputs {
            if !is_valid_dispatch_input_name(name) {
                return Err(cfg_err(
                    file,
                    "workflow.dispatch.inputs",
                    format!("bad_dispatch_input_name:{name}"),
                ));
            }
            if name == "suite" {
                return Err(cfg_err(
                    file,
                    "workflow.dispatch.inputs.suite",
                    "reserved_dispatch_input_name:suite",
                ));
            }
            validate_dispatch_input(name, input, file)?;
        }
        Ok(())
    }
}

/// Validate `workflow.preserve_workflows` entries.
///
/// # Errors
///
/// Returns [`ContractError::Config`] for duplicates, malformed paths, or generator-owned files.
pub fn validate_preserve_workflows(entries: &[String], file: &str) -> Result<(), ContractError> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        let Some(filename) = entry.strip_prefix(".github/workflows/") else {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        };
        let Some(slug) = filename.strip_suffix(".yml") else {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        };
        if !is_valid_workflow_slug(slug) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("bad_preserve_workflow:{entry}"),
            ));
        }
        if GENERATOR_OWNED_WORKFLOW_FILES.contains(&filename) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("cannot_preserve_generator_workflow:{entry}"),
            ));
        }
        if !seen.insert(entry.as_str()) {
            return Err(cfg_err(
                file,
                "workflow.preserve_workflows",
                format!("duplicate_preserve_workflow:{entry}"),
            ));
        }
    }
    Ok(())
}

fn validate_suite_id<'a>(
    id: &'a str,
    seen_ids: &mut BTreeSet<&'a str>,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    let valid = !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(cfg_err(
            file,
            format!("{prefix}.id"),
            format!("bad_suite_id:{id}"),
        ));
    }
    if RESERVED_SUITE_IDS.contains(&id) {
        return Err(cfg_err(
            file,
            format!("{prefix}.id"),
            format!("reserved_suite_id:{id}"),
        ));
    }
    if !seen_ids.insert(id) {
        return Err(cfg_err(
            file,
            format!("{prefix}.id"),
            format!("duplicate_suite_id:{id}"),
        ));
    }
    Ok(())
}

fn validate_suite_scalars(
    suite: &RustSuiteConfig,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if !is_valid_ascii_ident_with_hyphen(&suite.profile) {
        return Err(cfg_err(
            file,
            format!("{prefix}.profile"),
            format!("bad_suite_profile:{}", suite.profile),
        ));
    }
    if !(1..=MAX_SUITE_PARTITIONS).contains(&suite.partitions) {
        return Err(cfg_err(
            file,
            format!("{prefix}.partitions"),
            format!("bad_suite_partitions:{}", suite.partitions),
        ));
    }
    if !crate::workflow::timeout::JobTimeout::is_valid_minutes(u32::from(suite.timeout_minutes)) {
        return Err(cfg_err(
            file,
            format!("{prefix}.timeout_minutes"),
            format!("bad_suite_timeout:{}", suite.timeout_minutes),
        ));
    }
    if let Some(threads) = suite.test_threads
        && !(1..=MAX_SUITE_TEST_THREADS).contains(&threads)
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.test_threads"),
            format!("bad_suite_test_threads:{threads}"),
        ));
    }
    if let Some(label) = &suite.runner_label
        && !RUNNER_LABEL_CATALOG.contains(&label.as_str())
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.runner_label"),
            format!("unsupported_label:{label}"),
        ));
    }
    if let Some(workflow) = &suite.workflow {
        if !is_valid_workflow_slug(workflow) {
            return Err(cfg_err(
                file,
                format!("{prefix}.workflow"),
                format!("bad_suite_workflow:{workflow}"),
            ));
        }
        if RESERVED_SUITE_WORKFLOW_SLUGS.contains(&workflow.as_str()) {
            return Err(cfg_err(
                file,
                format!("{prefix}.workflow"),
                format!("reserved_suite_workflow:{workflow}"),
            ));
        }
    }
    Ok(())
}

fn validate_suite_triggers_and_schedule(
    suite: &RustSuiteConfig,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if suite.triggers.is_empty() {
        return Err(cfg_err(
            file,
            format!("{prefix}.triggers"),
            "empty_suite_triggers",
        ));
    }
    let mut trigger_set = BTreeSet::new();
    for trigger in &suite.triggers {
        if !trigger_set.insert(*trigger) {
            return Err(cfg_err(
                file,
                format!("{prefix}.triggers"),
                format!("duplicate_suite_trigger:{}", trigger.as_str()),
            ));
        }
    }
    if trigger_set.contains(&RustSuiteTrigger::Schedule) {
        if suite.schedule.is_empty() {
            return Err(cfg_err(
                file,
                format!("{prefix}.schedule"),
                "empty_suite_schedule",
            ));
        }
        for cron in &suite.schedule {
            ScheduleTrigger::single(cron.clone())
                .validate()
                .map_err(|_| {
                    cfg_err(
                        file,
                        format!("{prefix}.schedule"),
                        format!("bad_cron:{cron}"),
                    )
                })?;
        }
    } else if !suite.schedule.is_empty() {
        return Err(cfg_err(
            file,
            format!("{prefix}.schedule"),
            "schedule_requires_schedule_trigger",
        ));
    }
    if suite.cancel_in_progress == Some(true) && suite.has_schedule_trigger() {
        return Err(cfg_err(
            file,
            format!("{prefix}.cancel_in_progress"),
            "schedule_suite_cannot_cancel_in_progress",
        ));
    }
    if suite.required
        && (!trigger_set.contains(&RustSuiteTrigger::PullRequest)
            || !trigger_set.contains(&RustSuiteTrigger::Push))
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.required"),
            "required_suite_requires_pr_and_push_triggers",
        ));
    }
    if suite.allow_failure && suite.required {
        return Err(cfg_err(
            file,
            format!("{prefix}.allow_failure"),
            "allow_failure_cannot_be_required",
        ));
    }
    Ok(())
}
