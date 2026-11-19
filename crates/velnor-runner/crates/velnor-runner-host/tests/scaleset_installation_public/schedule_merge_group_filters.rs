// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::{HostError, WorkflowEvent, build_job_authority_policy_json};

use super::{
    SOURCE_SHA, TestInventoryContext, TestResult, inventory_json_for_contexts, sample_connection,
};

#[test]
fn public_job_authority_policy_refuses_uninterpretable_schedule_and_merge_group_filters()
-> TestResult<()> {
    let connection = sample_connection(
        vec![WorkflowEvent::Schedule, WorkflowEvent::MergeGroup],
        Vec::new(),
    )?;
    for event in ["schedule", "merge_group"] {
        let valid = inventory_json_for_contexts(vec![TestInventoryContext {
            event,
            branch_filters: vec!["main"],
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        }])?;
        let policy_json =
            build_job_authority_policy_json(&connection, &valid, SOURCE_SHA, &"d".repeat(64), 1)?;
        assert!(policy_json.contains("refs/heads/main"));

        let contexts = [
            (Vec::new(), vec!["v9.9.9"]),
            (vec!["main"], vec!["v9.9.9"]),
            (vec!["release"], Vec::new()),
            (vec!["ma*"], Vec::new()),
        ];
        for (branch_filters, tag_filters) in contexts {
            let inventory = inventory_json_for_contexts(vec![TestInventoryContext {
                event,
                branch_filters,
                tag_filters,
                pull_request_base_branches: Vec::new(),
            }])?;
            assert!(matches!(
                build_job_authority_policy_json(
                    &connection,
                    &inventory,
                    SOURCE_SHA,
                    &"d".repeat(64),
                    1
                ),
                Err(HostError::Config)
            ));
        }
    }
    Ok(())
}
