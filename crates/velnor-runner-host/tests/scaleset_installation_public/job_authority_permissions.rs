// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_runner_host::{HostError, WorkflowEvent, build_job_authority_policy_json};

use super::inventory_fixture::inventory_json;
use super::{SOURCE_SHA, TestInventoryContext, TestResult, sample_connection};

fn policy(permission: &'static str) -> Result<String, HostError> {
    let connection =
        sample_connection(vec![WorkflowEvent::Push], Vec::new()).map_err(|_| HostError::Config)?;
    let inventory = inventory_json(
        vec![TestInventoryContext {
            event: "push",
            branch_filters: vec!["main"],
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        }],
        BTreeMap::from([(permission, "read")]),
    )
    .map_err(|_| HostError::Config)?;
    build_job_authority_policy_json(&connection, &inventory, SOURCE_SHA, &"d".repeat(64), 1)
}

#[test]
fn public_job_authority_policy_admits_the_documented_token_permission_names() -> TestResult<()> {
    for permission in [
        "actions",
        "artifact-metadata",
        "attestations",
        "checks",
        "code-quality",
        "contents",
        "vulnerability-alerts",
    ] {
        policy(permission)?;
    }
    assert!(matches!(
        policy("no-such-permission"),
        Err(HostError::Config)
    ));
    Ok(())
}
