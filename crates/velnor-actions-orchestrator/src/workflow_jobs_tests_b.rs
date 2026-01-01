//! Workflow job constructor tests, second half.
//!
//! Declared via `#[path]` from `workflow_jobs.rs` under `cfg(test)`.

use super::workflow_jobs_tests::{assert_request_before, needs};
use super::*;
use velnor_actions_contract::StepKind;

#[test]
fn lint_job_installs_exact_actionlint_tools_before_exec() -> Result<(), Box<dyn std::error::Error>>
{
    let job = lint_job("ubuntu-26.04", &ToolCatalog::pinned())?;
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        ["Checkout", PREPARE_PINNED_TOOLS_STEP, "Run actionlint"]
    );
    let StepKind::Shell { run, .. } = &job.steps[1].kind else {
        return Err("pinned preparation must be a shell step".into());
    };
    let install_at = run.iter().position(|argument| argument == "install");
    let installed = install_at.map(|at| run[at + 1..].to_vec());
    assert_eq!(
        installed,
        Some(vec![
            "actionlint@1.7.12".to_owned(),
            "shellcheck@0.11.0".to_owned()
        ])
    );
    assert_eq!(job.steps[2].role, Some(StepRole::Actionlint));
    Ok(())
}

#[test]
fn pure_tofu_plan_drops_all_rust_setup() {
    use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;

    use crate::source_prep::FETCH_SOURCES_STEP;
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        PlanJobToolNeeds {
            opentofu: true,
            ..needs(PlanRustNeed::None)
        },
        &[],
    )
    .expect("plan job");
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&PREPARE_RUST_COMPONENTS_STEP),
        "no components step: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with(FETCH_SOURCES_STEP)),
        "no {FETCH_SOURCES_STEP}: {names:?}"
    );
    let prepare_at = names
        .iter()
        .position(|name| *name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let StepKind::Shell { run, env } = &job.steps[prepare_at].kind else {
        panic!("prepare must be a shell step: {names:?}");
    };
    let install_at = run
        .iter()
        .position(|arg| arg == "install")
        .expect("install argv");
    let specs = &run[install_at + 1..];
    assert_eq!(
        specs,
        [
            catalog.tool_spec(PinnedTool::Actionlint),
            catalog.tool_spec(PinnedTool::Shellcheck),
            catalog.tool_spec(PinnedTool::Zizmor),
            catalog.tool_spec(PinnedTool::Opentofu),
        ]
        .as_slice(),
        "pure-tofu plan installs opentofu plus validators: {run:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(!env.contains_key(key), "prepare carries no {key}: {env:?}");
    }
    assert_eq!(
        env.get("MISE_NO_CONFIG").map(String::as_str),
        Some("1"),
        "isolation overlay stays: {env:?}"
    );
}

#[test]
fn final_job_writes_request_before_merge() {
    let catalog = ToolCatalog::pinned();
    for acquire in [None, Some(checkout_action().expect("checkout step"))] {
        let job = final_job("ubuntu-26.04", &["rust-demo".to_owned()], acquire, &catalog)
            .expect("final job");
        assert_request_before(
            &job,
            "Merge reports",
            "write-request-v1:merge-v1",
            MERGE_OPERATION,
        );
    }
}

#[test]
fn final_job_needs_plan_crates_and_lint() {
    let catalog = ToolCatalog::pinned();
    let job = final_job(
        "ubuntu-26.04",
        &["rust-demo".to_owned(), "rust-nested".to_owned()],
        None,
        &catalog,
    )
    .expect("final job");
    assert_eq!(
        job.needs,
        [
            PLAN_JOB_ID.to_owned(),
            "rust-demo".to_owned(),
            "rust-nested".to_owned(),
            LINT_JOB_ID.to_owned(),
        ]
    );
    let job = final_job("ubuntu-26.04", &[], None, &catalog).expect("final job");
    assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()]);
}
