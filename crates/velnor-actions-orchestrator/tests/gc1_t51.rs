// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/config_workflow.rs"]
mod config_workflow;

use std::error::Error;
use std::io;

use velnor_actions_contract::config::{
    default_crate_timeout_minutes, default_dependency_seed_timeout_minutes,
    default_plan_timeout_minutes,
};
use velnor_actions_contract::{
    ContractError, GeneratorValidation, PullRequestCachePolicy, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_orchestrator::{HARD_CEILING_BYTES, MIN_RESERVE_BYTES, OPERATING_CEILING_BYTES};

use self::config_workflow::PartialWorkflow;

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

fn consumer_defaults() -> WorkflowConfig {
    PartialWorkflow::default().materialize()
}

fn with_policy(policy: PullRequestCachePolicy) -> WorkflowConfig {
    let mut config = consumer_defaults();
    config.pull_request_cache_policy = policy;
    config
}

fn check_consumer_defaults_preserved() -> Result<(), Box<dyn Error>> {
    let config = consumer_defaults();
    if config.name != "CI" {
        return Err(fail("workflow name default drifted"));
    }
    if config.policy != WorkflowPolicy::ConsumerV1 {
        return Err(fail("workflow policy default drifted"));
    }
    if config.generator_validation != GeneratorValidation::Bootstrap {
        return Err(fail("generator validation default drifted"));
    }
    if config.max_parallel_jobs != 2 {
        return Err(fail("max parallel jobs default drifted"));
    }
    if config.plan_timeout_minutes != default_plan_timeout_minutes()
        || config.crate_timeout_minutes != default_crate_timeout_minutes()
        || config.dependency_seed_timeout_minutes != default_dependency_seed_timeout_minutes()
    {
        return Err(fail("workflow timeout defaults drifted"));
    }
    if !config.mise_quiet || config.checkout_lfs {
        return Err(fail("workflow execution flags drifted"));
    }
    if config.pull_request_cache_policy != PullRequestCachePolicy::ReadOnly {
        return Err(fail("default cache policy left the read-only baseline"));
    }
    if config.default_branch.is_some()
        || config.runner_label.is_some()
        || config.tofu_apply.is_some()
        || config.review_completion.is_some()
    {
        return Err(fail("optional product configuration gained a default"));
    }
    if !config.tasks.is_empty()
        || !config.task_suites.is_empty()
        || !config.preserve_workflows.is_empty()
    {
        return Err(fail("task configuration gained a default"));
    }
    if !config.verify.is_empty() || !config.lint.is_default() || !config.dispatch.is_empty() {
        return Err(fail("verification configuration gained a default"));
    }
    config
        .validate("consumer-defaults")
        .map_err(|error| fail(format!("default consumer config rejected: {error}")))?;
    Ok(())
}

fn check_single_github_only_policy() -> Result<(), Box<dyn Error>> {
    if WorkflowConfig::GITHUB_CACHE_ACCOUNTING_CEILING_BYTES != HARD_CEILING_BYTES {
        return Err(fail("accounting ceiling left the admission ceiling"));
    }
    if WorkflowConfig::GITHUB_CACHE_DEFAULT_OPERATING_CEILING_BYTES != OPERATING_CEILING_BYTES {
        return Err(fail("operating ceiling left the admission ceiling"));
    }
    if WorkflowConfig::GITHUB_CACHE_MINIMUM_RESERVE_BYTES != MIN_RESERVE_BYTES {
        return Err(fail("minimum reserve left the admission reserve"));
    }
    if WorkflowConfig::GITHUB_CACHE_DEFAULT_OPERATING_CEILING_BYTES
        + WorkflowConfig::GITHUB_CACHE_MINIMUM_RESERVE_BYTES
        != WorkflowConfig::GITHUB_CACHE_ACCOUNTING_CEILING_BYTES
    {
        return Err(fail(
            "operating ceiling plus reserve missed the accounting ceiling",
        ));
    }
    Ok(())
}

#[test]
fn gc1_t51_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_consumer_defaults_preserved()?;
    check_single_github_only_policy()?;
    let readonly: WorkflowConfig =
        toml::from_str::<PartialWorkflow>("pull_request_cache_policy = \"read-only\"\n")
            .map_err(|error| fail(format!("read-only policy rejected at parse: {error}")))?
            .materialize();
    if readonly.pull_request_cache_policy != PullRequestCachePolicy::ReadOnly {
        return Err(fail("read-only policy lost its spelling"));
    }
    readonly
        .validate("read-only")
        .map_err(|error| fail(format!("read-only consumer config rejected: {error}")))?;
    Ok(())
}

fn check_rejects_obsolete_pr_write() -> Result<(), Box<dyn Error>> {
    let config = with_policy(PullRequestCachePolicy::SameRepositoryScoped);
    match config.validate("pr-write") {
        Err(ContractError::Config {
            key_path, problem, ..
        }) => {
            if key_path != "workflow.pull_request_cache_policy" {
                return Err(fail("pr-write rejection pointed at the wrong key"));
            }
            if !problem.contains("obsolete_pr_write_policy") {
                return Err(fail(format!(
                    "pr-write rejection lost its reason: {problem}"
                )));
            }
            Ok(())
        }
        Err(error) => Err(fail(format!(
            "pr-write failed with the wrong error: {error}"
        ))),
        Ok(()) => Err(fail("obsolete pr-write policy validated")),
    }
}

fn check_rejects_undeclared_cache_knobs() -> Result<(), Box<dyn Error>> {
    for document in [
        "pull_request_cache_policy = \"same-repository-scoped\"\n",
        "operating_ceiling_bytes = 9000000000\n",
        "paid_overflow = true\n",
        "mbx_persist = true\n",
        "legacy_cache = true\n",
    ] {
        let parsed: Result<PartialWorkflow, _> = toml::from_str(document);
        match parsed {
            Err(_) => {}
            Ok(config) => {
                if config.materialize().validate("undeclared-knob").is_ok() {
                    return Err(fail(format!(
                        "undeclared cache knob validated: {}",
                        document.trim()
                    )));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn gc1_t51_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_obsolete_pr_write()?;
    check_rejects_undeclared_cache_knobs()?;
    Ok(())
}
