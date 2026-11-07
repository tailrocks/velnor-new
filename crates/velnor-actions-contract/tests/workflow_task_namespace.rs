// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use velnor_actions_contract::config::WorkflowConfig;

#[test]
fn user_workflow_tasks_cannot_claim_the_internal_candidate_identity() -> Result<(), Box<dyn Error>>
{
    for runner in ["linux-x64", "macos-arm64"] {
        workflow("job-image-candidate-check", runner)?.validate("config.toml")?;
        let error = workflow("job-image-candidate", runner)?
            .validate("config.toml")
            .err()
            .ok_or("the internal candidate identity was admitted as a user workflow task")?;
        assert!(
            error
                .to_string()
                .contains("reserved_job_image_candidate_identity")
        );
    }
    Ok(())
}

fn workflow(id: &str, runner: &str) -> Result<WorkflowConfig, serde_json::Error> {
    serde_json::from_value(serde_json::json!({
        "name": "CI",
        "policy": "consumer-v1",
        "default_branch": "main",
        "generator_validation": "bootstrap",
        "max_parallel_jobs": 2,
        "tasks": [{
            "kind": "verification",
            "id": id,
            "mise_task": "verify",
            "source": {
                "mise_config": ".velnor/verification/mise.toml",
                "working_directory": ".velnor/verification"
            },
            "runner": runner,
            "timeout_minutes": 5
        }]
    }))
}
