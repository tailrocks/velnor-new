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
    assert!(execution.mode.is_none());
    assert!(
        execution
            .workflows
            .contains(&velnor_actions_contract::RoutingWorkflow::Qualification)
    );

    velnor_actions_workflow_renderer::render::test_render_capture::clear();
    let tree =
        velnor_actions_workflow_renderer::render::test_render_capture::with_full_tree_capture(
            || crate::render_staged_tree_with(&preparation, None),
        )?;
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take()
        .ok_or("render boundary did not record a workflow")?;
    assert!(capture.canonical.len() > 500_000);
    assert!(capture.selected.len() <= 500_000);
    std::fs::write(output.join("canonical.yml"), &capture.canonical)?;
    std::fs::write(output.join("selected.yml"), &capture.selected)?;
    assert!(
        velnor_actions_workflow_renderer::render::test_render_capture::workflow_yaml_is_semantically_equal(
            &capture.canonical,
            &capture.selected,
        )?
    );
    assert_eq!(
        crate::validate_shell::staged_run_signatures_for_equivalence(&capture.canonical)?,
        crate::validate_shell::staged_run_signatures(&capture.selected)?,
        "compact sharing must preserve ordered shell bodies and dialects"
    );
    crate::validate::validate_staged(&tree)?;
    assert!(tree.get(".github/workflows/ci.yml").is_some());
    Ok(())
}
