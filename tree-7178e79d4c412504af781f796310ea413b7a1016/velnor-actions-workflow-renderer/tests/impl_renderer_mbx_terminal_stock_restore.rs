//! The roundtrip terminal job alone certifies exact stock restore evidence.

use std::collections::BTreeSet;

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::schema2::QUALIFICATION_WORKFLOW;
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, RenderError, Schema2WorkflowRequest, render_schema2_workflows,
};

use super::impl_renderer_fixtures::mise;

fn qualification() -> Result<String, RenderError> {
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

#[test]
fn terminal_certifier_is_always_run_and_depends_only_on_roundtrip_roles() -> Result<(), RenderError>
{
    let text = qualification()?;
    let terminal = job_block(&text, "mbx-cache-roundtrip-terminal")?;
    assert!(terminal.contains("always() && (inputs.mode == 'mbx-cache-roundtrip'"));
    assert!(terminal.contains("github.ref_protected == true"));
    let needs = terminal
        .split_once("    needs:\n")
        .and_then(|(_, rest)| rest.split_once("    permissions:").map(|(needs, _)| needs))
        .ok_or_else(|| RenderError::InvalidWorkflow("terminal_needs_missing".to_owned()))?;
    let dependencies: Vec<&str> = needs
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(|id| id.trim_matches('"'))
        .collect();
    assert_eq!(
        dependencies,
        vec![
            "mbx-cache-write-hosted",
            "mbx-cache-read-hosted",
            "mbx-cache-corrupt-import-hosted"
        ]
    );
    assert!(terminal.contains("actions: read"));
    assert!(!terminal.contains("contents: read"));
    assert!(terminal.contains("name: Prepare private MBX terminal evidence"));
    assert!(terminal.contains("id: prepare_terminal_evidence"));
    assert!(
        terminal
            .contains("mkdir -m 700 \"$root/writer\" \"$root/reader\" \"$root/corrupt-reader\"")
    );
    assert!(terminal.contains(
        "for directory in \"$root\" \"$root/writer\" \"$root/reader\" \"$root/corrupt-reader\" \"$stock\""
    ));
    assert!(terminal.contains("stock_restore_private_dir \"$directory\" \"$RUNNER_TEMP\""));
    assert_eq!(
        terminal
            .matches("always() && steps.prepare_terminal_evidence.outcome == 'success'")
            .count(),
        4
    );
    assert!(terminal.contains("name: Classify MBX stock restore outcomes"));
    assert!(terminal.contains("name: Upload MBX terminal certification"));
    assert!(terminal.contains("mbx-cache-evidence-mbx-cache-write-hosted-r"));
    assert!(terminal.contains("mbx-cache-evidence-mbx-cache-read-hosted-r"));
    assert!(terminal.contains("mbx-cache-evidence-mbx-cache-corrupt-import-hosted-r"));
    assert!(terminal.contains("-r${{ github.run_id }}-a${{ github.run_attempt }}"));
    assert!(
        terminal.contains(
            "mbx-cache-roundtrip-terminal-r${{ github.run_id }}-a${{ github.run_attempt }}"
        )
    );
    assert!(terminal.contains("Cache restored from key:"));
    assert!(terminal.contains("Cache not found for input keys:"));
    assert!(terminal.contains("to_entries"));
    assert!(terminal.contains("FailedToRestore"));
    assert!(terminal.contains("stock_restore_classify_receipt"));
    assert!(terminal.contains("jq -c -e -s"));
    assert!(terminal.contains("then .[0] else error(\"expected one object\") end"));
    assert!(terminal.contains("--max-redirs 0"));
    assert!(!terminal.contains("--location"));
    Ok(())
}
