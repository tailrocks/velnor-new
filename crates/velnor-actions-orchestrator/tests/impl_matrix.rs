//! Crate-graph cases: shape, static parallelism, plan agreement, identity.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{FinalStatus, Plan};
use velnor_actions_orchestrator::{
    GenerateOptions, generate, merge_internal, merge_passed, plan_internal, prepare,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, passing_reports, plan_for,
    plan_for_source_change,
};
use crate::impl_merge::{merge, merge_request, success_jobs};

/// Preview `ci.yml` text for one config; temps keep the dirs alive.
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
    let text = fs::read_to_string(preview.join(WORKFLOW_PATH))?;
    Ok((repo, parent, text))
}

#[test]
fn crate_yaml_shape_exact() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for line in [
        "  rust-demo:",
        "    name: Rust / demo",
        "  required:",
        "  plan:",
        "  actionlint:",
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
fn rust_jobs_ignore_matrix_cap() -> TestResult {
    // T18 flip of `crate_graph_ignores_matrix_cap`: rust-only repos
    // stay static (same assertions, scoped name); tofu root jobs
    // honor the cap (next test).
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 3\n";
    let repo = make_repo(config)?;
    let prep = prepare(repo.path())?;
    assert!(plan_for(&prep)?.contains("1 Rust crate job"), "crate plan");
    let (_repo, _parent, text) = preview_yml(config)?;
    for marker in ["max-parallel:", "strategy:", "fromJSON"] {
        assert!(
            !text.contains(marker),
            "static yaml keeps {marker}:\n{text}"
        );
    }
    Ok(())
}

/// Preview `ci.yml` text for one config plus extra `files`.
fn preview_yml_with(
    config: &str,
    files: &[(&str, &str)],
) -> Result<(TempDir, TempDir, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    for (relative, content) in files {
        let target = repo.path().join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let text = fs::read_to_string(preview.join(WORKFLOW_PATH))?;
    Ok((repo, parent, text))
}

#[test]
fn tofu_root_jobs_honor_matrix_cap() -> TestResult {
    // T18 flip partner: the configured cap renders on every tofu
    // root job while the rust job stays static and markers scrub.
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\nmax_parallel_jobs = 3\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\"]\n";
    let (_repo, _parent, text) = preview_yml_with(
        config,
        &[
            ("stacks/a/main.tf", "variable \"a\" {}\n"),
            ("stacks/b/main.tf", "variable \"b\" {}\n"),
        ],
    )?;
    assert_eq!(
        text.matches("max-parallel: 3").count(),
        2,
        "both tofu jobs declare the cap, the rust job none:\n{text}"
    );
    for marker in [
        "VELNOR_MATRIX_NEEDS_JOB",
        "VELNOR_MATRIX_OUTPUT",
        "VELNOR_MATRIX_MAX_PARALLEL",
        "fromJSON",
    ] {
        assert!(!text.contains(marker), "marker {marker}:\n{text}");
    }
    Ok(())
}

#[test]
fn plan_crate_agreement() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let plan = plan_for(&prepare(repo.path())?)?;
    assert!(plan.contains("1 Rust crate job"), "crate plan:\n{plan}");
    assert!(plan.contains("rust-demo"), "crate entry:\n{plan}");
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    assert!(text.contains("  rust-demo:"), "crate yaml:\n{text}");
    assert!(text.contains("name: Rust / demo"), "display yaml:\n{text}");
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let repo = make_repo(ignored)?;
    let plan = plan_for(&prepare(repo.path())?)?;
    assert!(plan.contains("no-work workflow"), "static plan:\n{plan}");
    assert!(!plan.contains("Rust crate job"), "static plan:\n{plan}");
    assert!(plan.contains("no matrix fan-out"), "static plan:\n{plan}");
    let (_repo, _parent, text) = preview_yml(ignored)?;
    assert!(!text.contains("strategy:"), "static yaml:\n{text}");
    assert!(!text.contains("fromJSON"), "static yaml:\n{text}");
    Ok(())
}

#[test]
fn ignored_rust_merges_to_no_work_never_passes() -> TestResult {
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let repo = make_repo(ignored)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": head,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    assert!(plan.task_ids.is_empty(), "ignored Rust plans zero work");
    assert!(plan.obligations.is_empty(), "ignored Rust plans zero work");
    assert!(
        plan.matrix.include.is_empty(),
        "ignored Rust plans zero legs"
    );

    // Zero obligations with green validators merge to no_work, and the
    // required gate stays red: no_work proves nothing validated.
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(&plan, &matrix, &serde_json::json!([]), &success_jobs());
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::NoWork);
    assert_eq!(
        final_report.expected_report_ids,
        [] as [std::string::String; 0]
    );
    assert_eq!(final_report.counts.selected, 0);
    assert_eq!(final_report.counts.executed, 0);
    assert_eq!(final_report.counts.covered, 0);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.blocked, 0);
    assert_eq!(final_report.counts.not_run, 0);
    let merged = merge_internal(&request.to_string())?;
    assert!(
        merge_passed(&merged)?,
        "no work with valid required checks passes"
    );

    // Contrast: the same fixture without the ignore selects obligations
    // whose passing reports merge to passed with a green gate.
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "fixture must select work");
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &plan,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    let merged = merge_internal(&request.to_string())?;
    assert!(merge_passed(&merged)?, "passing work greens the gate");
    Ok(())
}

#[test]
fn obligations_carry_fixed_identity() -> TestResult {
    let (_repo, _parent, text) = preview_yml(config_with_branch())?;
    for want in [
        "- name: Clippy",
        "VELNOR_TASK_ID: stack/rust/root/clippy/default",
        "VELNOR_TASK_DIGEST: b3-",
        "VELNOR_MATRIX_ID: ",
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
