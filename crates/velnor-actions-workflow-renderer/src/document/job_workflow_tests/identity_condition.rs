use std::collections::BTreeSet;

use velnor_actions_contract::Step;

use crate::RenderError;

use super::*;

#[test]
fn relocated_child_serializes_only_the_exact_plan_input_condition_alias() -> Result<(), RenderError>
{
    let mut job = source_job();
    job.steps.insert(0, identity_step("ubuntu-26.04", None)?);
    let workflow = workflow(job.clone());
    let shared = lane_share(job);
    let relocations =
        render_job_workflow_relocations(&workflow, &shared, &context(), &BTreeSet::new())?;

    assert_eq!(relocations.len(), 1);
    let child = &relocations[0].expected_callee_file.bytes;
    assert!(
        child
            .contains("if: inputs.plan_result == 'success' && inputs.qualification_campaign == ''")
    );
    assert!(!child.contains(
        "if: needs.plan.result == 'success' && needs.plan.outputs.qualification_campaign == ''"
    ));
    Ok(())
}

#[test]
fn inline_parent_keeps_original_identity_condition_and_rejects_child_alias()
-> Result<(), RenderError> {
    let mut job = source_job();
    job.steps.clear();
    let mut identity = identity_step("ubuntu-26.04", None)?;
    identity.condition = Some(crate::cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION.to_owned());
    job.steps.insert(0, identity.clone());
    let workflow_ir = workflow(job.clone());
    let mut shared = lane_share(job);
    shared.job_workflows.clear();

    let parent =
        crate::document::workflow_to_yaml(&workflow_ir, &shared, &context(), &BTreeSet::new())?;
    assert!(yaml_contains(
        &parent,
        crate::cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION
    ));
    assert!(!yaml_contains(
        &parent,
        crate::cache_p08::TOOLS_CACHE_RELOCATED_RUN_CONDITION
    ));

    identity.condition = Some(crate::cache_p08::TOOLS_CACHE_RELOCATED_RUN_CONDITION.to_owned());
    let mut malformed_job = source_job();
    malformed_job.steps.clear();
    malformed_job.steps.insert(0, identity);
    let malformed_workflow = workflow(malformed_job.clone());
    let mut malformed_shared = lane_share(malformed_job);
    malformed_shared.job_workflows.clear();
    assert!(
        crate::document::workflow_to_yaml(
            &malformed_workflow,
            &malformed_shared,
            &context(),
            &BTreeSet::new(),
        )
        .is_err()
    );
    Ok(())
}

fn identity_step(runs_on: &str, source_sha: Option<&str>) -> Result<Step, RenderError> {
    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@8418fedb1110381a0d3ba51a7a415d7c1b884550".to_owned(),
        version: "2026.10.4".to_owned(),
        sha256: "a".repeat(64),
    };
    let tools = vec!["rust@1.98.1".to_owned()];
    let payload = crate::cache_p08::ToolsCachePayload::new(crate::cache_p08::ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs: &tools,
        rustup_toolchain: Some("1.98.1"),
        rustup_components: &[],
        scale_set_image_source_sha: source_sha,
    })?;
    payload.runtime_prelude_step()
}

fn yaml_contains(value: &Yaml, expected: &str) -> bool {
    match value {
        Yaml::Map(fields) => fields
            .iter()
            .any(|(_, child)| yaml_contains(child, expected)),
        Yaml::Seq(values) => values.iter().any(|child| yaml_contains(child, expected)),
        Yaml::Str(text) => text == expected,
        _ => false,
    }
}
