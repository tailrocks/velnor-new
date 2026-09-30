//! Crate-graph cases: shape, static parallelism, plan agreement, identity.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, plan_text, prepare};

use super::impl_common::{TestResult, config_with_branch, make_repo, plan_for_source_change};

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
fn crate_yaml_shape_exact() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for line in [
        "  rust-demo:",
        "    name: Rust / demo",
        "  velnor-final:",
        "  velnor-plan:",
        "  velnor-workflow-lint:",
        "      - rust-demo",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    for marker in [
        "strategy:",
        "fromJSON",
        "max-parallel:",
        "velnor-task",
        "VELNOR_MATRIX_NEEDS_JOB",
        "VELNOR_MATRIX_OUTPUT",
        "VELNOR_MATRIX_MAX_PARALLEL",
    ] {
        assert!(!text.contains(marker), "matrix remnant {marker}:\n{text}");
    }
    Ok(())
}

#[test]
fn crate_graph_ignores_matrix_cap() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 3\n";
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    assert!(plan_text(&prep).contains("1 Rust crate job"), "crate plan");
    let (_repo, _parent, text) = preview_yml(config)?;
    for marker in ["max-parallel:", "strategy:", "fromJSON"] {
        assert!(
            !text.contains(marker),
            "static yaml keeps {marker}:\n{text}"
        );
    }
    Ok(())
}

#[test]
fn plan_crate_agreement() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let plan = plan_text(&prepare(repo.path())?);
    assert!(plan.contains("1 Rust crate job"), "crate plan:\n{plan}");
    assert!(plan.contains("rust-demo"), "crate entry:\n{plan}");
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    assert!(text.contains("  rust-demo:"), "crate yaml:\n{text}");
    assert!(text.contains("name: Rust / demo"), "display yaml:\n{text}");
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let repo = make_repo(ignored)?;
    let plan = plan_text(&prepare(repo.path())?);
    assert!(plan.contains("no-work workflow"), "static plan:\n{plan}");
    assert!(!plan.contains("Rust crate job"), "static plan:\n{plan}");
    assert!(plan.contains("no matrix fan-out"), "static plan:\n{plan}");
    let (_repo, _parent, text) = preview_yml(ignored)?;
    assert!(!text.contains("strategy:"), "static yaml:\n{text}");
    assert!(!text.contains("fromJSON"), "static yaml:\n{text}");
    Ok(())
}

#[test]
fn obligations_carry_fixed_identity() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for want in [
        "- name: Clippy",
        "VELNOR_TASK_ID: stack/rust/root/clippy/default",
        "VELNOR_TASK_DIGEST: b3-",
        "VELNOR_MATRIX_KEY: m-",
        "cargo clippy --locked --offline",
    ] {
        assert!(text.contains(want), "missing {want}:\n{text}");
    }
    assert!(
        !text.contains("${{ matrix."),
        "fixed identities, no matrix context:\n{text}"
    );
    Ok(())
}

#[test]
fn planned_entries_carry_leg_command_and_digest() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty(), "fixture must select work");
    for entry in &plan.matrix.include {
        assert!(!entry.run.is_empty(), "empty run for {}", entry.id);
        assert!(
            !entry.run.contains(['\n', '\r', '\0']),
            "multiline run for {}",
            entry.id
        );
        assert!(
            entry.run.starts_with("mise "),
            "unpinned run for {}: {}",
            entry.id,
            entry.run
        );
        let obligation = plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .ok_or_else(|| std::io::Error::other("entry without obligation"))?;
        assert_eq!(
            entry.task_digest, obligation.task_digest,
            "digest mismatch for {}",
            entry.id
        );
    }
    Ok(())
}
