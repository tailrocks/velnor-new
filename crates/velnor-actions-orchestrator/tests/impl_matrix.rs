//! Matrix strategy cases: shape, max-parallel, plan agreement, consumption.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, plan_text, prepare};

use super::impl_common::{TestResult, config_with_branch, make_repo};

/// Preview `velnor.yml` text for one config; temps keep the dirs alive.
fn preview_yml(config: &str) -> Result<(TempDir, TempDir, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let text = fs::read_to_string(preview.join(".github/workflows/velnor.yml"))?;
    Ok((repo, parent, text))
}

#[test]
fn matrix_yaml_shape_exact() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for line in [
        "    strategy:",
        "      fail-fast: false",
        "      max-parallel: 2",
        "      matrix: ${{ fromJSON(needs.velnor-plan.outputs.matrix) }}",
        "    outputs:",
        "      matrix: ${{ steps.plan.outputs.matrix }}",
        "        id: plan",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    assert!(!text.contains("VELNOR_MATRIX_"), "marker stripped:\n{text}");
    Ok(())
}

#[test]
fn max_parallel_honored() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 3\n";
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    assert!(
        plan_text(&prep).contains("up to 3 matrix entries"),
        "plan cap"
    );
    let (_repo, _parent, text) = preview_yml(config)?;
    assert!(text.contains("max-parallel: 3"), "yaml cap:\n{text}");
    Ok(())
}

#[test]
fn plan_matrix_agreement() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    assert!(
        plan_text(&prepare(repo.path())?).contains("Rust crate matrix"),
        "matrix plan"
    );
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    assert!(text.contains("strategy:"), "matrix yaml:\n{text}");
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let repo = make_repo(ignored)?;
    let plan = plan_text(&prepare(repo.path())?);
    assert!(plan.contains("no-work workflow"), "static plan:\n{plan}");
    assert!(!plan.contains("Rust crate matrix"), "static plan:\n{plan}");
    assert!(plan.contains("no matrix fan-out"), "static plan:\n{plan}");
    let (_repo, _parent, text) = preview_yml(ignored)?;
    assert!(!text.contains("strategy:"), "static yaml:\n{text}");
    assert!(!text.contains("fromJSON"), "static yaml:\n{text}");
    Ok(())
}

#[test]
fn task_consumes_matrix_context() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for want in [
        "- name: Run task",
        "VELNOR_TASK_ID: ${{ matrix.task_id }}",
        "VELNOR_TASK_RUN: ${{ matrix.run }}",
        "$VELNOR_TASK_RUN",
        ":?matrix.run_missing",
    ] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
    assert!(!text.contains("(default)"), "no per-group steps:\n{text}");
    Ok(())
}
