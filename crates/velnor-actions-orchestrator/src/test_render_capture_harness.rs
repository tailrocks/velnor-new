//! Exact-consumer diagnostic harness. This test is deliberately ignored and
//! runs only with the private `test-render-capture` feature enabled.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};
use velnor_actions_workflow_renderer::render::test_render_capture::{
    RenderCapture, SourceJobWorkflowCapture,
};
use velnor_actions_workflow_renderer::render::{RenderedTree, test_render_capture};

#[path = "test_render_capture_harness_census.rs"]
mod census;

type CensusResult = Option<Result<SourceJobWorkflowCapture, String>>;
type HarnessResult<T> = Result<T, Box<dyn Error>>;
type ClassifiedRender = (RenderCapture, Option<RenderedTree>, Option<String>);
type CapturedRender = (
    RenderCapture,
    Option<RenderedTree>,
    Option<String>,
    CensusResult,
);

#[test]
#[ignore = "requires VELNOR_CAPTURE_ROOT and external VELNOR_CAPTURE_OUT"]
fn capture_exact_consumer_marked_workflow_before_size_guard() -> Result<(), Box<dyn Error>> {
    let (root, output) = capture_paths()?;
    let output = initialize_output(&root, &output)?;
    let preparation = crate::prepare(&root)?;
    let dispatch = capture_dispatch()?;
    validate_capture_configuration(&preparation)?;
    let source_ids = source_ids_for_mode(&preparation, dispatch)?;
    let (capture, tree, diagnostic_error, child_capture) =
        render_capture(&preparation, dispatch, source_ids.as_ref())?;
    write_capture_and_check_parity(&output, &capture)?;
    finish_capture(
        &output,
        source_ids.as_ref(),
        child_capture,
        capture.selected.len(),
        tree,
        diagnostic_error,
    )
}

fn capture_paths() -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let root = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_ROOT").ok_or("missing root")?);
    let output = PathBuf::from(std::env::var_os("VELNOR_CAPTURE_OUT").ok_or("missing output")?);
    Ok((root.canonicalize()?, output))
}

fn initialize_output(root: &Path, output: &Path) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    let output = output.canonicalize()?;
    if output.starts_with(root) {
        return Err("capture output must be outside the consumer checkout".into());
    }
    std::fs::write(output.join("capture-status.txt"), "incomplete\n")?;
    Ok(output)
}

fn capture_dispatch() -> Result<Option<ExecutionMode>, Box<dyn Error>> {
    match std::env::var("VELNOR_CAPTURE_MODE") {
        Ok(mode) => Ok(Some(ExecutionMode::parse(&mode)?)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn validate_capture_configuration(
    preparation: &crate::prepare::GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let execution = preparation
        .config
        .execution
        .as_ref()
        .ok_or("missing execution config")?;
    assert!(execution.mode.is_none());
    assert!(
        execution
            .workflows
            .contains(&RoutingWorkflow::Qualification)
    );
    Ok(())
}

fn source_ids_for_mode(
    preparation: &crate::prepare::GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<Option<BTreeSet<String>>, Box<dyn Error>> {
    if dispatch == Some(ExecutionMode::Both) {
        Ok(Some(census::typed_rust_lane_job_ids(preparation)?))
    } else {
        Ok(None)
    }
}

fn render_capture(
    preparation: &crate::prepare::GenerationPreparation,
    dispatch: Option<ExecutionMode>,
    source_ids: Option<&BTreeSet<String>>,
) -> HarnessResult<CapturedRender> {
    let (rendered, child_capture) = census::render_with_source_capture(source_ids, || {
        crate::render_staged_tree_with(preparation, dispatch)
    });
    let capture = test_render_capture::take();
    classify_render(rendered, capture, dispatch)
        .map(|(capture, tree, error)| (capture, tree, error, child_capture))
}

fn classify_render(
    rendered: Result<RenderedTree, crate::error::OrchestratorError>,
    capture: Option<RenderCapture>,
    dispatch: Option<ExecutionMode>,
) -> HarnessResult<ClassifiedRender> {
    match rendered {
        Ok(tree) => {
            let capture = capture.ok_or("render boundary did not record a workflow")?;
            if dispatch.is_none() {
                assert!(capture.canonical.len() > 500_000);
            }
            assert!(capture.selected.len() <= 500_000);
            Ok((capture, Some(tree), None))
        }
        Err(error) if dispatch == Some(ExecutionMode::Both) => {
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
            Ok((capture, None, Some(expected_error)))
        }
        Err(error) => Err(error.into()),
    }
}

fn finish_capture(
    output: &Path,
    source_ids: Option<&BTreeSet<String>>,
    child_capture: CensusResult,
    selected_bytes: usize,
    tree: Option<RenderedTree>,
    diagnostic_error: Option<String>,
) -> Result<(), Box<dyn Error>> {
    if let Some(source_ids) = source_ids {
        census::write_source_job_workflow_census(
            output,
            source_ids,
            child_capture,
            selected_bytes,
        )?;
    } else if child_capture.is_some() {
        return Err("source-job census ran without an explicit Both selector".into());
    }
    write_capture_outcome(output, tree, diagnostic_error, source_ids.is_some())
}

fn write_capture_outcome(
    output: &Path,
    tree: Option<RenderedTree>,
    diagnostic_error: Option<String>,
    has_source_census: bool,
) -> Result<(), Box<dyn Error>> {
    match (tree, diagnostic_error) {
        (Some(tree), None) => {
            crate::validate::validate_staged(&tree)?;
            assert!(tree.get(".github/workflows/ci.yml").is_some());
            std::fs::write(output.join("capture-status.txt"), "validated\n")?;
        }
        (None, Some(error)) => {
            std::fs::write(
                output.join("capture-status.txt"),
                format!(
                    "diagnostic-only\n{error}\nsource-job-census: {}\n",
                    if has_source_census {
                        "source-bound extracted children validated; complete workflow graph unavailable before size guard"
                    } else {
                        "not requested"
                    }
                ),
            )?;
        }
        _ => return Err("render capture outcome is inconsistent".into()),
    }
    Ok(())
}

fn write_capture_and_check_parity(
    output: &Path,
    capture: &RenderCapture,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(output.join("canonical.yml"), &capture.canonical)?;
    std::fs::write(output.join("selected.yml"), &capture.selected)?;
    assert!(test_render_capture::workflow_yaml_is_semantically_equal(
        &capture.canonical,
        &capture.selected,
    )?);
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
