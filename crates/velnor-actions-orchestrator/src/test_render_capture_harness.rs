//! Exact-consumer diagnostic harness. This test is deliberately ignored and
//! runs only with the private `test-render-capture` feature enabled.

use std::path::PathBuf;

#[test]
#[ignore = "requires VELNOR_CAPTURE_ROOT and external VELNOR_CAPTURE_OUT"]
fn capture_exact_consumer_marked_workflow_before_size_guard()
-> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_ROOT").ok_or("missing root")?);
    let output = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_OUT").ok_or("missing output")?);
    let root = root.canonicalize()?;
    std::fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    if output.starts_with(&root) {
        return Err("capture output must be outside the consumer checkout".into());
    }

    let preparation = crate::prepare(&root)?;
    let execution = preparation
        .config
        .execution
        .as_ref()
        .ok_or("missing execution config")?;
    assert_eq!(
        execution.mode,
        Some(velnor_actions_contract::ExecutionMode::Both)
    );
    assert!(
        execution
            .workflows
            .contains(&velnor_actions_contract::RoutingWorkflow::Qualification)
    );

    velnor_actions_workflow_renderer::render::test_render_capture::clear();
    let tree = crate::render_staged_tree_with(&preparation, None)?;
    crate::validate::validate_staged(&tree)?;
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take()
        .ok_or("render boundary did not record a workflow")?;
    std::fs::write(output.join("canonical.yml"), capture.canonical)?;
    std::fs::write(output.join("selected.yml"), capture.selected)?;
    assert!(tree.get(".github/workflows/ci.yml").is_some());
    Ok(())
}
