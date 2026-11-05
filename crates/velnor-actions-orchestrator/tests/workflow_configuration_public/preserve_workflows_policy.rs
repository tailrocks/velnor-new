// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_orchestrator::{OrchestratorError, prepare};

use super::workflow_configuration_fixture;

#[test]
fn consumer_policy_rejects_preserved_workflows() -> Result<(), Box<dyn Error>> {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\npolicy = \"consumer-v1\"\npreserve_workflows = [\".github/workflows/project-checks.yml\"]\n";
    let repo = workflow_configuration_fixture::make_repo(config)?;
    match prepare(repo.path()) {
        Err(OrchestratorError::Config {
            key_path, problem, ..
        }) => {
            assert_eq!(key_path, "workflow.preserve_workflows");
            assert_eq!(problem, "preserve_workflows_not_allowed_under_consumer_v1");
            Ok(())
        }
        other => Err(io::Error::other(format!(
            "expected the consumer-v1 preserve_workflows rejection, got {other:?}"
        ))
        .into()),
    }
}
