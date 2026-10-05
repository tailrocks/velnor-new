use std::collections::BTreeMap;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_workflow_renderer::{RenderError, action_step, render_workflow_ir, shell_step};

use super::impl_renderer_fixtures::*;

#[test]
fn serialized_cache_writers_cannot_weaken_the_shared_gate() -> Result<(), RenderError> {
    let uses = format!("actions/cache/save@{}", "a".repeat(40));
    let unguarded = action_step("Unconditional cache save", &uses, BTreeMap::new())?;
    let mut weak = action_step("Always cache save", &uses, BTreeMap::new())?;
    weak.condition = Some("false || always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "cache",
            "Cache",
            Vec::new(),
            vec![unguarded, weak],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!("if: {CACHE_SAVE_CONDITION}")),
        "missing condition receives shared gate:\n{text}"
    );
    assert!(
        text.contains(&format!(
            "if: {CACHE_SAVE_CONDITION} && (false || always())"
        )),
        "caller condition cannot replace shared gate:\n{text}"
    );
    Ok(())
}

#[test]
fn serialized_mbx_exports_cannot_weaken_the_shared_gate() -> Result<(), RenderError> {
    let mut export = shell_step(
        "MBX export",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "mbx cache export --format directory bundle".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    export.condition = Some("false || always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![job("cache", "Cache", Vec::new(), vec![export])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!(
            "if: {CACHE_SAVE_CONDITION} && (false || always())"
        )),
        "caller condition cannot replace the shared MBX export gate:\n{text}"
    );
    Ok(())
}

#[test]
fn existing_protected_conjunctions_are_not_duplicated() -> Result<(), RenderError> {
    let uses = format!("actions/cache/save@{}", "b".repeat(40));
    let mut save = action_step("Image-eligible cache save", &uses, BTreeMap::new())?;
    let image_condition =
        format!("{CACHE_SAVE_CONDITION} && env.VELNOR_CACHE_IMAGE_ELIGIBLE == 'true'");
    save.condition = Some(image_condition.clone());
    let mut export = shell_step(
        "MBX bundle export",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "mbx cache export --format directory bundle".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let bundle_condition = format!(
        "{CACHE_SAVE_CONDITION} && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'"
    );
    export.condition = Some(bundle_condition.clone());
    let text = render_workflow_ir(
        &fixture_ir(vec![job("cache", "Cache", Vec::new(), vec![save, export])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains(&format!("if: {image_condition}")), "{text}");
    assert!(text.contains(&format!("if: {bundle_condition}")), "{text}");
    assert_eq!(text.matches(CACHE_SAVE_CONDITION).count(), 2, "{text}");
    Ok(())
}

#[test]
fn a_disjunctive_cache_condition_cannot_hide_behind_the_gate_prefix() -> Result<(), RenderError> {
    let uses = format!("actions/cache/save@{}", "c".repeat(40));
    let mut save = action_step("Disjunctive cache save", &uses, BTreeMap::new())?;
    let original = format!("{CACHE_SAVE_CONDITION} && false || always()");
    save.condition = Some(original.clone());
    let text = render_workflow_ir(
        &fixture_ir(vec![job("cache", "Cache", Vec::new(), vec![save])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!("if: {CACHE_SAVE_CONDITION} && ({original})")),
        "disjunction must remain behind the shared gate:\n{text}"
    );
    Ok(())
}
