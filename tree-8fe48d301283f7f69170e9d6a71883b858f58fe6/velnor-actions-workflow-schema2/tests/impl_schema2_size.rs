//! Schema-2 size cap: the direct renderer enforces the workflow byte cap.

use std::collections::BTreeSet;
use velnor_actions_contract_config::RoutingWorkflow;
use velnor_actions_workflow_generator::{MbxQualificationPins, Schema2WorkflowRequest};
use velnor_actions_workflow_schema2::render_schema2_workflows;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::setup::MiseSetup;
use velnor_actions_workflow_tree::MAX_WORKFLOW_BYTES;

const VERSION: &str = "0.1.0";

#[test]
fn direct_schema2_renderer_rejects_an_oversized_workflow() -> Result<(), RenderError> {
    let large_mbx_version = format!("{}.0.0", "9".repeat(MAX_WORKFLOW_BYTES / 4));
    let request = Schema2WorkflowRequest {
        version: VERSION.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: MiseSetup {
                uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
                version: "2026.9.18".to_owned(),
                sha256: "a".repeat(64),
            },
            candidate_action_uses:
                "jdx/mr-boxington-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            mbx_version: large_mbx_version,
            rust_version: "1.98.0".to_owned(),
        }),
        generator_release: None,
    };
    let error =
        render_schema2_workflows(&request).expect_err("direct routing render must enforce the cap");
    assert!(
        error
            .to_string()
            .contains("workflow_too_large:.github/workflows/qualification.yml")
    );
    Ok(())
}
