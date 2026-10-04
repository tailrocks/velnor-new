//! Registered qualification jobs preserve cancellation identity and permissions.

use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW;
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, RenderError, Schema2WorkflowRequest, render_schema2_workflows,
};

use super::impl_renderer_fixtures::mise;

fn qualification_workflow() -> Result<String, RenderError> {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: mise(),
            mbx_action_uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            mbx_version: "1.22.0".to_owned(),
            rust_version: "1.98.1".to_owned(),
        }),
    };
    render_schema2_workflows(&request)?
        .into_iter()
        .find(|file| file.path == QUALIFICATION_WORKFLOW)
        .map(|file| file.bytes)
        .ok_or_else(|| RenderError::InvalidWorkflow("missing_qualification_workflow".to_owned()))
}

fn job_block<'a>(text: &'a str, id: &str) -> Result<&'a str, RenderError> {
    let start = text
        .find(&format!("  {id}:\n"))
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("missing_qualification_job:{id}")))?;
    let content_start = start + 2;
    let suffix = &text[content_start..];
    let end = suffix
        .match_indices("\n  ")
        .find_map(|(offset, _)| {
            suffix
                .as_bytes()
                .get(offset + 3)
                .is_some_and(|byte| *byte != b' ')
                .then_some(offset)
        })
        .unwrap_or(suffix.len());
    Ok(&text[start..content_start + end])
}

fn assert_cancel_gate(block: &str, mode: &str, always: bool) {
    let gate = format!(
        "inputs.mode == '{mode}' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"
    );
    assert!(block.contains(&gate), "{mode}: {block}");
    if always {
        assert!(
            block.contains(&format!("always() && ({gate})")),
            "{mode}: {block}"
        );
    }
}

fn assert_minimal_permission(block: &str, actions: &str) {
    for permission in [
        actions,
        "contents: none",
        "pull-requests: none",
        "id-token: none",
    ] {
        assert!(block.contains(permission), "missing {permission}: {block}");
    }
    assert!(!block.contains("id-token: write"), "{block}");
    assert!(!block.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN"), "{block}");
    assert!(!block.contains("secrets."), "{block}");
}

#[test]
fn both_cancellation_phases_render_all_six_trusted_mode_jobs() -> Result<(), RenderError> {
    let workflow = qualification_workflow()?;
    for (phase, token) in [("pre-save", "pre-save"), ("during-save", "during-save")] {
        for (role, mode_role, permission, always) in [
            ("victim", "victim", "actions: write", false),
            ("controller", "controller", "actions: write", false),
            ("observer", "controller", "actions: read", true),
        ] {
            let id = format!("mbx-cancel-{phase}-{role}");
            let block = job_block(&workflow, &id)?;
            let mode = format!("mbx-cancel-{token}-{mode_role}");
            assert_cancel_gate(block, &mode, always);
            assert_minimal_permission(block, permission);
            assert!(
                block.contains("name: Upload MBX cancellation receipt"),
                "{block}"
            );
            assert!(block.contains("retention-days: 1"), "{block}");
        }
    }
    Ok(())
}

#[test]
fn cancellation_receipts_keep_run_identity_and_observers_use_r13_cache_reader()
-> Result<(), RenderError> {
    let workflow = qualification_workflow()?;
    assert!(workflow.contains("inputs.probe_id"), "{workflow}");
    assert!(
        workflow.contains("format('MBX cancellation {0} {1}', inputs.mode, inputs.probe_id)"),
        "{workflow}"
    );

    for (phase, victim_artifact, controller_artifact, observer_artifact) in [
        (
            "pre-save",
            "mbx-cancel-victim-pre-save",
            "mbx-cancel-controller-receipt-pre-save",
            "mbx-cancel-observer-pre-save",
        ),
        (
            "during-save",
            "mbx-cancel-victim-during-save",
            "mbx-cancel-controller-receipt-during-save",
            "mbx-cancel-observer-during-save",
        ),
    ] {
        let victim = job_block(&workflow, &format!("mbx-cancel-{phase}-victim"))?;
        let controller = job_block(&workflow, &format!("mbx-cancel-{phase}-controller"))?;
        let observer = job_block(&workflow, &format!("mbx-cancel-{phase}-observer"))?;
        assert!(
            victim.contains(&format!("name: {victim_artifact}")),
            "{victim}"
        );
        assert!(
            controller.contains(&format!("name: {controller_artifact}")),
            "{controller}"
        );
        assert!(
            observer.contains(&format!("name: {observer_artifact}")),
            "{observer}"
        );
        assert!(controller.contains(".run_attempt == 1"), "{controller}");
        assert!(controller.contains("workflow_run_id"), "{controller}");
        assert!(observer.contains(".run_attempt == 1"), "{observer}");
        assert!(observer.contains("child_run_id"), "{observer}");
        assert!(observer.contains("child_attempt"), "{observer}");
        for step in [
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
        ] {
            assert!(
                observer.contains(step),
                "missing R13 observer step {step}: {observer}"
            );
        }
    }
    Ok(())
}
