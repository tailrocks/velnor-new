// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `execution.overrides` for control jobs: opt-in placement on the scale set
//! and the fork fence every moved job carries.

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::json;
use velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION;
use velnor_actions_contract::workflow::{
    CacheAccess, Concurrency, Job, JobEnv, JobTimeout, PermissionLevel, Permissions, Step,
    StepKind, Trigger, WorkflowIr, expand_workflow_with_lane_plan, fork_fenced_condition,
    scale_set_support_jobs,
};
use velnor_actions_contract::{ContractError, VelnorConfig};

const SCALE: &str = "scale-set:velnor+ubuntu-26.04-scale-set";

fn config(overrides: &serde_json::Value) -> Result<VelnorConfig, Box<dyn Error>> {
    let config: VelnorConfig = serde_json::from_value(json!({
        "schema": 2,
        "workflow": {
            "name": "CI",
            "policy": "velnor-repository-v1",
            "generator_validation": "bootstrap",
            "max_parallel_jobs": 2
        },
        "resources": { "compiler_process_budget": 2, "test_process_budget": 2 },
        "test_sharding": { "default_shards": 1 },
        "stacks": {},
        "discovery": {},
        "execution": {
            "default_profile": "local",
            "hosted_profile": "hosted",
            "scale_set_profile": "local",
            "profiles": {
                "hosted": { "kind": "github-hosted", "label": "ubuntu-26.04", "platform": "linux/amd64" },
                "local": {
                    "kind": "github-scale-set",
                    "name": "ubuntu-26.04-scale-set",
                    "labels": ["velnor", "ubuntu-26.04-scale-set"],
                    "platform": "linux/amd64"
                }
            },
            "overrides": overrides
        }
    }))?;
    Ok(config)
}

fn job(name: &str, needs: &[&str], condition: Option<&str>) -> Job {
    Job {
        display_name: name.to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: needs.iter().map(|id| (*id).to_owned()).collect(),
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_access: CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: vec![Step {
            name: "Run".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    }
}

fn workflow() -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions {
            contents: PermissionLevel::Read,
            ..Permissions::default()
        },
        concurrency: Concurrency::writer_queue("ci"),
        jobs: BTreeMap::from([
            ("plan".to_owned(), job("Plan", &[], None)),
            (
                "jactionlint".to_owned(),
                job("Jactionlint", &["plan"], None),
            ),
            (
                "required".to_owned(),
                job("Required verdict", &["plan"], Some("${{ !cancelled() }}")),
            ),
        ]),
    }
}

fn runs_on(config: &VelnorConfig, id: &str) -> Result<String, Box<dyn Error>> {
    let expanded = expand_workflow_with_lane_plan(&workflow(), config, None)?;
    Ok(expanded
        .ir
        .jobs
        .get(id)
        .ok_or("job missing")?
        .runs_on
        .clone())
}

fn moved(id: &str) -> serde_json::Value {
    json!({ id: { "profile": "local", "role": "control" } })
}

#[test]
fn control_jobs_stay_hosted_without_an_override() -> Result<(), Box<dyn Error>> {
    let config = config(&json!({}))?;
    for id in ["plan", "jactionlint", "required"] {
        assert_eq!(runs_on(&config, id)?, "ubuntu-26.04", "{id}");
    }
    assert!(scale_set_support_jobs(&config).is_empty());
    Ok(())
}

#[test]
fn control_override_moves_only_the_named_job() -> Result<(), Box<dyn Error>> {
    let config = config(&moved("plan"))?;
    assert_eq!(runs_on(&config, "plan")?, SCALE);
    assert_eq!(runs_on(&config, "jactionlint")?, "ubuntu-26.04");
    assert_eq!(runs_on(&config, "required")?, "ubuntu-26.04");
    Ok(())
}

#[test]
fn hosted_override_keeps_a_control_job_hosted() -> Result<(), Box<dyn Error>> {
    let config = config(&json!({ "plan": { "profile": "hosted", "role": "control" } }))?;
    assert_eq!(runs_on(&config, "plan")?, "ubuntu-26.04");
    Ok(())
}

#[test]
fn moved_control_jobs_keep_their_check_names() -> Result<(), Box<dyn Error>> {
    let config = config(&moved("required"))?;
    let expanded = expand_workflow_with_lane_plan(&workflow(), &config, None)?;
    let required = expanded.ir.jobs.get("required").ok_or("required missing")?;
    assert_eq!(required.display_name, "Required verdict");
    Ok(())
}

#[test]
fn control_override_with_a_verification_role_is_rejected() -> Result<(), Box<dyn Error>> {
    let config = config(&json!({ "plan": { "profile": "local", "role": "verification" } }))?;
    let error = expand_workflow_with_lane_plan(&workflow(), &config, None)
        .err()
        .ok_or("role mismatch accepted")?;
    assert!(error.to_string().contains("role_not_eligible"), "{error}");
    Ok(())
}

#[test]
fn renderer_added_control_jobs_are_override_keys() -> Result<(), Box<dyn Error>> {
    let ids = ["alint", "cargo-deny", "cargo-machete", "required-gate"];
    let overrides: serde_json::Map<String, serde_json::Value> = ids
        .iter()
        .map(|id| {
            (
                (*id).to_owned(),
                json!({ "profile": "local", "role": "control" }),
            )
        })
        .collect();
    let config = config(&serde_json::Value::Object(overrides))?;
    expand_workflow_with_lane_plan(&workflow(), &config, None)?;
    assert_eq!(
        scale_set_support_jobs(&config)
            .into_iter()
            .collect::<Vec<_>>(),
        ids.map(str::to_owned)
    );
    let wrong_role =
        self::config(&json!({ "alint": { "profile": "local", "role": "verification" } }))?;
    let result: Result<_, ContractError> =
        expand_workflow_with_lane_plan(&workflow(), &wrong_role, None);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn moved_control_jobs_are_fenced_from_fork_pull_requests() -> Result<(), Box<dyn Error>> {
    let config = config(&json!({
        "plan": { "profile": "local", "role": "control" },
        "jactionlint": { "profile": "local", "role": "control" },
        "required": { "profile": "local", "role": "control" }
    }))?;
    let expanded = expand_workflow_with_lane_plan(&workflow(), &config, None)?;
    for id in ["plan", "jactionlint", "required"] {
        let condition = expanded
            .ir
            .jobs
            .get(id)
            .ok_or("job missing")?
            .condition
            .clone();
        let condition = condition.ok_or_else(|| format!("{id} carries no fork fence"))?;
        assert!(
            condition.ends_with(&format!("({EPHEMERAL_CHECK_ADMISSION_CONDITION})"))
                || condition == EPHEMERAL_CHECK_ADMISSION_CONDITION,
            "{id}: {condition}"
        );
    }
    let required = expanded.ir.jobs.get("required").ok_or("required missing")?;
    assert_eq!(
        required.condition.as_deref(),
        Some(format!("(!cancelled()) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})").as_str())
    );
    Ok(())
}

#[test]
fn hosted_control_jobs_get_no_fence() -> Result<(), Box<dyn Error>> {
    let config = config(&json!({}))?;
    let expanded = expand_workflow_with_lane_plan(&workflow(), &config, None)?;
    assert_eq!(expanded.ir.jobs.get("plan").ok_or("plan")?.condition, None);
    assert_eq!(
        expanded
            .ir
            .jobs
            .get("required")
            .ok_or("required")?
            .condition
            .as_deref(),
        Some("${{ !cancelled() }}")
    );
    Ok(())
}

#[test]
fn the_fence_admits_only_trusted_events_and_never_widens() {
    assert_eq!(
        fork_fenced_condition(None),
        EPHEMERAL_CHECK_ADMISSION_CONDITION
    );
    assert_eq!(
        fork_fenced_condition(Some("always()")),
        format!("(always()) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})")
    );
    let once = fork_fenced_condition(Some("${{ always() }}"));
    assert_eq!(fork_fenced_condition(Some(&once)), once);
    for forbidden in ["pull_request_target", "|| true"] {
        assert!(!once.contains(forbidden));
    }
    assert!(
        EPHEMERAL_CHECK_ADMISSION_CONDITION
            .contains("github.event.pull_request.head.repo.full_name == github.repository")
    );
}
