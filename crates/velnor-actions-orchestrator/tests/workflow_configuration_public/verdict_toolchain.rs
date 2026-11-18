// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[test]
fn verdict_setup_does_not_request_rust_homes() -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::{CheckoutHistory, StepKind};
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    use velnor_actions_workflow_renderer::native_job_builders::final_job;

    for version in ["1.98.0", "1.99.1"] {
        let catalog = ToolCatalog::pinned().with_project_tool(PinnedTool::Rust, version)?;
        let job = final_job(
            "ubuntu-26.04",
            &[],
            None,
            &catalog,
            CheckoutHistory::ShallowOnPush,
        )
        .map_err(|error| format!("{error:?}"))?;
        for step in &job.steps {
            if let StepKind::Shell { env, .. } = &step.kind {
                for key in ["RUSTUP_TOOLCHAIN", "RUSTUP_HOME", "CARGO_HOME"] {
                    assert!(!env.contains_key(key), "verdict setup retained {key}");
                }
            }
        }
    }
    Ok(())
}
