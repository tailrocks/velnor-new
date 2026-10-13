//! Released MBX action ownership must exclude Velnor's hosted bundle route.

use velnor_actions_contract::{StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

#[test]
fn released_action_owns_hosted_cache_and_gates_manual_route() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mut mbx = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let StepKind::Action { uses, with, env } = &mut mbx[1].kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_action_missing".to_owned(),
        ));
    };
    *uses = format!(
        "jdx/mr-boxington-action@{}",
        "d0825fbaf3cc36ca2609aa38e71046265a1f1e37"
    );
    with.insert(
        "isolate-objects-cache".to_owned(),
        "${{ runner.environment == 'github-hosted' }}".to_owned(),
    );
    env.insert(
        "ACTIONS_CACHE_MODE".to_owned(),
        "${{ runner.environment == 'github-hosted' && 'write' || 'read' }}".to_owned(),
    );
    let built = job("demo", "MBX job", Vec::new(), mbx.into());
    let text = render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;

    let init = text
        .find("name: Initialize private MBX store")
        .expect("private store init");
    let action = text
        .find("name: Restore MBX objects")
        .expect("released MBX action");
    assert!(text[init..action].contains("if: runner.environment != 'github-hosted'"));
    assert!(text[action..].contains("d0825fbaf3cc36ca2609aa38e71046265a1f1e37"));
    assert!(
        text[action..]
            .contains("isolate-objects-cache: ${{ runner.environment == 'github-hosted' }}")
    );
    assert!(text[action..].contains(
        "ACTIONS_CACHE_MODE: ${{ runner.environment == 'github-hosted' && 'write' || 'read' }}"
    ));

    let names = [
        "Prepare MBX bundle key",
        "Restore MBX single bundle",
        "Import MBX single bundle",
        "Export MBX single bundle",
        "Save MBX single bundle",
    ];
    for (index, name) in names.iter().enumerate() {
        let start = text
            .find(&format!("name: {name}"))
            .expect("manual bundle step");
        let end = names
            .get(index + 1)
            .and_then(|next| text[start + 1..].find(&format!("name: {next}")))
            .map(|offset| start + 1 + offset)
            .unwrap_or(text.len());
        let block = &text[start..end];
        assert!(
            block.contains("if: runner.environment != 'github-hosted'"),
            "hosted manual step {name} is not gated: {block}"
        );
    }
    Ok(())
}
