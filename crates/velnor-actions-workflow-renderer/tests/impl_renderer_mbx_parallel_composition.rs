//! The generated MBX parallel probe keeps its seed, readers, and new writer composable.

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

fn assert_one_need(block: &str, dependency: &str) {
    assert!(
        block.contains(&format!("    needs:\n      - {dependency}\n")),
        "expected only {dependency} dependency:\n{block}"
    );
}

fn assert_reader_import(block: &str) {
    for evidence in [
        "name: Restore MBX single bundle",
        "name: Import MBX single bundle",
        "name: Require imported MBX objects",
        "mbx cache stats --json",
        ".objects > 0",
        "name: Require reused compilation",
        ".savings.cached_compilations > 0",
    ] {
        assert!(block.contains(evidence), "{evidence}:\n{block}");
    }
}

#[test]
fn readers_and_independent_writer_run_concurrently_from_the_seed() -> Result<(), RenderError> {
    let text = qualification_workflow()?;
    let seed = job_block(&text, "mbx-parallel-seed")?;
    assert!(!seed.contains("    needs:"), "{seed}");
    assert!(seed.contains("actions: write"), "{seed}");

    for role in [
        "mbx-parallel-reader-a",
        "mbx-parallel-reader-b",
        "mbx-parallel-new-key-writer",
    ] {
        assert_one_need(job_block(&text, role)?, "mbx-parallel-seed");
    }

    for role in ["mbx-parallel-reader-a", "mbx-parallel-reader-b"] {
        let reader = job_block(&text, role)?;
        assert!(reader.contains("actions: read"), "{reader}");
        assert_reader_import(reader);
    }
    let new_writer = job_block(&text, "mbx-parallel-new-key-writer")?;
    assert!(new_writer.contains("qualification-mbx-v1/parallel/new-key"));
    assert!(new_writer.contains("actions: write"));
    assert!(new_writer.contains("name: Save MBX single bundle"));
    Ok(())
}

#[test]
fn parallel_observers_join_only_after_all_four_producer_roles() -> Result<(), RenderError> {
    let text = qualification_workflow()?;
    for role in ["mbx-parallel-observer-shared", "mbx-parallel-observer-new"] {
        let observer = job_block(&text, role)?;
        let needs = observer
            .split_once("    needs:\n")
            .and_then(|(_, tail)| tail.split_once("    permissions:").map(|(needs, _)| needs))
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("missing_needs:{role}")))?;
        let dependencies: Vec<&str> = needs
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- "))
            .map(|id| id.trim_matches('"'))
            .collect();
        assert_eq!(
            dependencies,
            [
                "mbx-parallel-seed",
                "mbx-parallel-reader-a",
                "mbx-parallel-reader-b",
                "mbx-parallel-new-key-writer"
            ],
            "{role}: {observer}"
        );
        assert!(observer.contains("Validate MBX parallel REST timestamps"));
        assert!(observer.contains("actions: read"));
    }
    Ok(())
}
