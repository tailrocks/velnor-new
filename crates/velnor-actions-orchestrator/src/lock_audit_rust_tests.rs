//! Channel-manifest coverage replaces `no_checksums` for verified Rust installs.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, StepRole, Trigger, WorkflowIr,
};
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PinnedTool, ToolCatalog};

use crate::lock_audit::audit_prepare_installs_with_catalog;
use crate::workflow::verifier_actions::call_step;
use crate::workflow::{
    NOT_CATALOGUED_CODE, VERIFY_JACTIONLINT_STEP, VERIFY_RESTORED_RUST_STEP,
    VERIFY_RUST_MANIFEST_STEP,
};
use velnor_actions_contract::workflow::RUST_VERIFIER_USES;
use velnor_actions_workflow_renderer::verifier_composite::VERIFY_JACTIONLINT_USES;

fn step(name: &str, role: Option<StepRole>, run: &[&str]) -> Step {
    Step {
        parallel: false,
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        kind: StepKind::Shell {
            run: run.iter().map(|arg| (*arg).to_owned()).collect(),
            env: BTreeMap::new(),
        },
    }
}

fn ir(verified: bool) -> WorkflowIr {
    let mut steps = Vec::new();
    if verified {
        steps.push(call_step(
            VERIFY_RESTORED_RUST_STEP,
            RUST_VERIFIER_USES[0],
            Some(StepRole::VerifyRustManifest),
        ));
    }
    steps.push(step(
        PREPARE_PINNED_TOOLS_STEP,
        Some(StepRole::PreparePinnedTools),
        &["mise", "--no-config", "install", "rust@${VELNOR_TOOL_RUST}"],
    ));
    if verified {
        steps.push(call_step(
            VERIFY_RUST_MANIFEST_STEP,
            RUST_VERIFIER_USES[1],
            Some(StepRole::VerifyRustManifest),
        ));
    }
    let job = Job {
        services: Vec::new(),
        check_runner: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    };
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "g".to_owned(),
            cancel_in_progress: "c".to_owned(),
        },
        jobs: BTreeMap::from([("plan".to_owned(), job)]),
        cache_mode: None,
    }
}

fn catalog(rust: &str) -> ToolCatalog {
    ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, rust)
        .expect("exact")
}

const LOCK: &str =
    "lockfile_version = 3\n\n[[tools.rust]]\nversion = \"1.99.1\"\nbackend = \"core:rust\"\n";

#[test]
fn verified_catalogued_rust_is_not_a_no_checksums_gap() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir(true),
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    assert_eq!(outcome.recommendation, None);
}

#[test]
fn display_name_and_noop_shell_do_not_attest_to_verification() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let mut workflow = ir(false);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(step(
            VERIFY_RUST_MANIFEST_STEP,
            Some(StepRole::VerifyRustManifest),
            &["sh", "-c", "true"],
        ));
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("forged_verifier_call:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn reversed_rust_verifier_phase_calls_do_not_attest_to_install() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let mut workflow = ir(true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .swap(0, 1);
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("rust_verifier_sequence_invalid:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn rust_verifier_with_changed_environment_is_not_canonical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut workflow = ir(true);
    let installed = workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .last_mut()
        .expect("installed verifier");
    let StepKind::Action { env, .. } = &mut installed.kind else {
        panic!("canonical typed action");
    };
    env.insert("RUSTUP_HOME".to_owned(), String::new());
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("noncanonical_verifier_call:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn rust_verifier_with_unregistered_action_path_is_forged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut workflow = ir(true);
    let installed = workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .last_mut()
        .expect("installed verifier");
    let StepKind::Action { uses, .. } = &mut installed.kind else {
        panic!("canonical typed action");
    };
    *uses = format!("{}-extra", RUST_VERIFIER_USES[1]);
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("forged_verifier_call:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn conditional_rust_verifier_is_not_canonical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut workflow = ir(true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .last_mut()
        .expect("installed verifier")
        .condition = Some("false".to_owned());
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("noncanonical_verifier_call:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn unverified_rust_still_reports_no_checksums() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir(false),
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    let summary = outcome.recommendation.expect("gap");
    assert!(summary.contains("rust@1.99.1(no_checksums)"), "{summary}");
}

#[test]
fn verified_uncatalogued_rust_blocks_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut both = ir(true);
    let second = both.jobs["plan"].clone();
    both.jobs.insert("other".to_owned(), second);
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &both,
        "ubuntu-26.04",
        &[],
        &catalog("1.84.1"),
    );
    let hits: Vec<_> = outcome
        .blocking
        .iter()
        .filter(|line| line.starts_with(&format!("{NOT_CATALOGUED_CODE}:rust@1.84.1")))
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", outcome.blocking);
}

/// One plan job installing jactionlint, with or without `Verify jactionlint`.
fn jactionlint_ir(spec: &str, verified: bool) -> WorkflowIr {
    let mut workflow = ir(false);
    let job = workflow.jobs.get_mut("plan").expect("plan job");
    job.steps = vec![step(
        PREPARE_PINNED_TOOLS_STEP,
        Some(StepRole::PreparePinnedTools),
        &["mise", "--no-config", "install", spec],
    )];
    if verified {
        job.steps.push(call_step(
            VERIFY_JACTIONLINT_STEP,
            VERIFY_JACTIONLINT_USES,
            None,
        ));
    }
    workflow
}

#[test]
fn a_verified_catalogued_jactionlint_needs_no_lock_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let verified = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir(&spec, true),
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(verified.blocking.is_empty(), "{:?}", verified.blocking);
    assert_eq!(verified.recommendation, None);
    let unverified = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir(&spec, false),
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    let summary = unverified
        .recommendation
        .expect("gap without the verify step");
    assert!(summary.contains(&spec), "{summary}");
}

#[test]
fn a_verified_uncatalogued_jactionlint_still_needs_its_lock_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Jactionlint, "9.9.9")
        .expect("exact");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir("jactionlint@9.9.9", true),
        "ubuntu-26.04",
        &[],
        &catalog,
    );
    let summary = outcome
        .recommendation
        .expect("gap for an uncatalogued version");
    assert!(summary.contains("jactionlint@9.9.9"), "{summary}");
}

#[test]
fn jactionlint_verifier_must_follow_the_pinned_tool_install_immediately() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .insert(1, step("Unrelated step", None, &["echo", "between"]));
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("jactionlint_verifier_sequence_invalid:plan:")),
        "{:?}",
        outcome.blocking
    );
}
