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
    std::fs::write(output.join("capture-status.txt"), "incomplete\n")?;

    let preparation = crate::prepare(&root)?;
    let execution = preparation
        .config
        .execution
        .as_ref()
        .ok_or("missing execution config")?;
    let dispatch = match std::env::var("VELNOR_CAPTURE_MODE") {
        Ok(mode) => Some(velnor_actions_contract::ExecutionMode::parse(&mode)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    assert!(execution.mode.is_none());
    assert!(
        execution
            .workflows
            .contains(&velnor_actions_contract::RoutingWorkflow::Qualification)
    );

    velnor_actions_workflow_renderer::render::test_render_capture::clear();
    let rendered =
        velnor_actions_workflow_renderer::render::test_render_capture::with_full_tree_capture(
            || crate::render_staged_tree_with(&preparation, dispatch),
        );
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take();
    let (capture, tree, diagnostic_error) = match rendered {
        Ok(tree) => {
            let capture = capture.ok_or("render boundary did not record a workflow")?;
            if dispatch.is_none() {
                assert!(capture.canonical.len() > 500_000);
            }
            assert!(capture.selected.len() <= 500_000);
            (capture, Some(tree), None)
        }
        Err(error) if dispatch == Some(velnor_actions_contract::ExecutionMode::Both) => {
            let Some(capture) = capture else {
                return Err(error.into());
            };
            let expected_error = format!(
                "render: invalid workflow: workflow_too_large:.github/workflows/ci.yml:{}:500000",
                capture.selected.len()
            );
            if error.to_string() != expected_error {
                return Err(error.into());
            }
            assert!(capture.selected.len() > 500_000);
            (capture, None, Some(expected_error))
        }
        Err(error) => return Err(error.into()),
    };
    write_capture_and_check_parity(&output, &capture)?;
    match (tree, diagnostic_error) {
        (Some(tree), None) => {
            crate::validate::validate_staged(&tree)?;
            assert!(tree.get(".github/workflows/ci.yml").is_some());
            std::fs::write(output.join("capture-status.txt"), "validated\n")?;
        }
        (None, Some(error)) => {
            std::fs::write(
                output.join("capture-status.txt"),
                format!("diagnostic-only\n{error}\n"),
            )?;
        }
        _ => return Err("render capture outcome is inconsistent".into()),
    }
    Ok(())
}

fn write_capture_and_check_parity(
    output: &std::path::Path,
    capture: &velnor_actions_workflow_renderer::render::test_render_capture::RenderCapture,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(output.join("canonical.yml"), &capture.canonical)?;
    std::fs::write(output.join("selected.yml"), &capture.selected)?;
    assert!(
        velnor_actions_workflow_renderer::render::test_render_capture::workflow_yaml_is_semantically_equal(
            &capture.canonical,
            &capture.selected,
        )?
    );
    let selected_signatures = if capture.selected.len() <= 500_000 {
        crate::validate_shell::staged_run_signatures(&capture.selected)?
    } else {
        crate::validate_shell::staged_run_signatures_for_equivalence(&capture.selected)?
    };
    assert_eq!(
        crate::validate_shell::staged_run_signatures_for_equivalence(&capture.canonical)?,
        selected_signatures,
        "compact sharing must preserve ordered shell bodies and dialects"
    );
    Ok(())
}
