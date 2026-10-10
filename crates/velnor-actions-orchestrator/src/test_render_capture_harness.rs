//! Exact-consumer diagnostic harness. This test is deliberately ignored and
//! runs only with the private `test-render-capture` feature enabled.

use std::collections::BTreeSet;
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
    let rendered = crate::render_staged_tree_with(&preparation, None);
    let capture = velnor_actions_workflow_renderer::render::test_render_capture::take();
    let captured = capture.is_some();
    if let Some(capture) = &capture {
        let canonical_bytes = capture.canonical.len();
        let selected_bytes = capture.selected.len();
        std::fs::write(output.join("canonical.yml"), capture.canonical.as_bytes())?;
        std::fs::write(output.join("selected.yml"), capture.selected.as_bytes())?;
        let shared_root = output.join("shared");
        let mut shared_bytes = 0usize;
        let mut shared_paths = BTreeSet::new();
        for file in &capture.shared {
            if !shared_paths.insert(&file.path) {
                return Err(format!("duplicate shared action path: {}", file.path).into());
            }
            let safe = velnor_actions_workflow_renderer::guard::validate_tree_path(&file.path)?;
            let destination = shared_root.join(safe.as_str());
            let parent = destination.parent().ok_or("shared output has no parent")?;
            std::fs::create_dir_all(parent)?;
            std::fs::write(&destination, file.bytes.as_bytes())?;
            shared_bytes += file.bytes.len();
            eprintln!(
                "pre-cap shared file: path={} bytes={}",
                file.path,
                file.bytes.len()
            );
        }
        eprintln!(
            "pre-cap capture written: canonical_bytes={canonical_bytes} selected_bytes={selected_bytes} shared_files={} shared_bytes={shared_bytes}",
            capture.shared.len()
        );
    }
    let tree = match rendered {
        Ok(tree) => tree,
        Err(error)
            if captured
                && capture_overflow_matches(
                    &error.to_string(),
                    output.join("selected.yml").as_path(),
                )? =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if !captured {
        return Err("render boundary did not record a workflow".into());
    }
    crate::validate::validate_staged(&tree)?;
    assert!(tree.get(".github/workflows/ci.yml").is_some());
    let capture = capture.expect("capture was checked above");
    assert_eq!(
        tree.get(".github/workflows/ci.yml"),
        Some(capture.selected.as_str())
    );
    for file in &capture.shared {
        assert_eq!(tree.get(&file.path), Some(file.bytes.as_str()));
    }
    Ok(())
}

fn capture_overflow_matches(
    error: &str,
    selected_path: &std::path::Path,
) -> Result<bool, std::io::Error> {
    let selected_bytes = std::fs::read(selected_path)?.len();
    Ok(error.contains(&format!(
        "workflow_too_large:.github/workflows/ci.yml:{selected_bytes}:500000"
    )))
}
