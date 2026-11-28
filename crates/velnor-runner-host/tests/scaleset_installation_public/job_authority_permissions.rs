// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_runner_host::{HostError, WorkflowEvent, build_job_authority_policy_json};

use super::inventory_fixture::inventory_json_of;
use super::{SOURCE_SHA, TestInventoryContext, TestResult, sample_connection};

fn policy_of(permission: &'static str, workflows: usize, jobs: usize) -> Result<String, HostError> {
    let connection =
        sample_connection(vec![WorkflowEvent::Push], Vec::new()).map_err(|_| HostError::Config)?;
    let inventory = inventory_json_of(
        &[TestInventoryContext {
            event: "push",
            branch_filters: vec!["main"],
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        }],
        &BTreeMap::from([(permission, "read")]),
        workflows,
        jobs,
    )
    .map_err(|_| HostError::Config)?;
    build_job_authority_policy_json(&connection, &inventory, SOURCE_SHA, &"d".repeat(64), 1)
}

fn policy(permission: &'static str) -> Result<String, HostError> {
    policy_of(permission, 1, 1)
}

#[test]
fn the_host_admits_exactly_the_scopes_velnor_model_lists() -> TestResult<()> {
    for scope in velnor_model::token_permission_scopes() {
        policy(scope)?;
    }
    for scope in ["repository-projects", "no-such-permission"] {
        assert!(matches!(policy(scope), Err(HostError::Config)), "{scope}");
    }
    Ok(())
}

#[test]
fn the_host_workflow_limit_is_the_one_the_generator_applies() -> TestResult<()> {
    let limit = velnor_model::JOB_AUTHORITY_MAX_WORKFLOWS;
    policy_of("contents", limit, limit)?;
    assert!(matches!(
        policy_of("contents", limit + 1, limit + 1),
        Err(HostError::Config)
    ));
    Ok(())
}

#[test]
fn the_host_job_limit_is_the_one_the_generator_applies() -> TestResult<()> {
    let limit = velnor_model::JOB_AUTHORITY_MAX_JOBS;
    policy_of("contents", 2, limit)?;
    assert!(matches!(
        policy_of("contents", 2, limit + 1),
        Err(HostError::Config)
    ));
    Ok(())
}
