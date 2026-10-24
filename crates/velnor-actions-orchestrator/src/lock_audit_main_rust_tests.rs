//! Channel-manifest coverage replaces `no_checksums` for verified Rust installs.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, StepRole, Trigger, WorkflowIr,
};
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PinnedTool, ToolCatalog};

use crate::lock_audit::audit_prepare_installs_with_catalog;
use crate::workflow::NOT_CATALOGUED_CODE;

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

fn verifier_action(role: StepRole, uses: &str) -> Step {
    Step {
        parallel: false,
        name: "Neutral verifier label".to_owned(),
        id: None,
        role: Some(role),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn ir(verified: bool, catalog: &ToolCatalog) -> WorkflowIr {
    let mut steps = Vec::new();
    if verified {
        steps.push(restored_rust_verifier(catalog));
    }
    steps.push(step(
        PREPARE_PINNED_TOOLS_STEP,
        Some(StepRole::PreparePinnedTools),
        &["mise", "--no-config", "install", "rust@${VELNOR_TOOL_RUST}"],
    ));
    if verified {
        steps.push(installed_rust_verifier(catalog));
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
            workflow_call: false,
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

fn ir_with_verifier(verifier: Step) -> WorkflowIr {
    let mut workflow = ir(false, &ToolCatalog::pinned());
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(verifier);
    workflow
}

fn ir_with_restored_and_verifier(verifier: Step, catalog: &ToolCatalog) -> WorkflowIr {
    let mut workflow = ir(false, catalog);
    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    steps.insert(0, restored_rust_verifier(catalog));
    steps.push(verifier);
    workflow
}

fn restored_rust_verifier(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        &catalog.for_emission(),
        crate::workflow::ToolchainPhase::Restored,
    )
    .expect("production restored Rust shell verifier")
}

fn installed_rust_verifier(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        &catalog.for_emission(),
        crate::workflow::ToolchainPhase::Installed,
    )
    .expect("production installed Rust shell verifier")
}

fn jactionlint_verifier(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::jactionlint_verify_step::verifier_shell_step(
        &catalog.for_emission(),
    )
    .expect("production Jactionlint shell verifier")
}

fn catalog(rust: &str) -> ToolCatalog {
    ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, rust)
        .expect("The tool catalog must resolve.")
}

const LOCK: &str =
    "lockfile_version = 3\n\n[[tools.rust]]\nversion = \"1.99.1\"\nbackend = \"core:rust\"\n";

#[test]
fn verified_catalogued_rust_is_not_a_no_checksums_gap() {
    let dir = tempfile::tempdir().expect("The temporary directory must be created.");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("The lock file must be written.");
    let catalog = catalog("1.99.1");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir(true, &catalog),
        "ubuntu-26.04",
        &[],
        &catalog,
    );
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    assert_eq!(outcome.recommendation, None);
}

#[test]
fn prepare_audit_accepts_the_production_inline_installed_rust_step() {
    let dir = tempfile::tempdir().expect("temporary repository");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("The lock file must be written.");
    let catalog = catalog("1.99.1");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir_with_restored_and_verifier(installed_rust_verifier(&catalog), &catalog),
        "ubuntu-26.04",
        &[],
        &catalog,
    );
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    assert_eq!(outcome.recommendation, None);
}

#[test]
fn production_inline_installed_verifier_does_not_hide_unknown_rust() {
    let dir = tempfile::tempdir().expect("temporary repository");
    let catalog = catalog("1.84.1");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir_with_restored_and_verifier(installed_rust_verifier(&catalog), &catalog),
        "ubuntu-26.04",
        &[],
        &catalog,
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.starts_with(&format!("{NOT_CATALOGUED_CODE}:rust@1.84.1"))),
        "unknown Rust remains blocked: {:?}",
        outcome.blocking
    );
}

#[test]
fn restored_or_mutated_shell_verifier_does_not_cover_installed_rust() {
    let dir = tempfile::tempdir().expect("temporary repository");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("The lock file must be written.");
    let catalog = catalog("1.99.1");
    for mut verifier in [
        restored_rust_verifier(&catalog),
        installed_rust_verifier(&catalog),
    ] {
        if matches!(&verifier.kind, StepKind::Shell { .. })
            && verifier.name == crate::workflow::VERIFY_RUST_MANIFEST_STEP
        {
            let StepKind::Shell { run, .. } = &mut verifier.kind else {
                unreachable!("checked shell verifier")
            };
            run[2].push_str(" # changed");
        }
        let outcome = audit_prepare_installs_with_catalog(
            dir.path(),
            &ir_with_verifier(verifier),
            "ubuntu-26.04",
            &[],
            &catalog,
        );
        assert!(
            outcome
                .recommendation
                .as_deref()
                .is_some_and(|summary| summary.contains("rust@1.99.1(no_checksums)")),
            "restored or altered proof must not cover installed Rust: {outcome:?}"
        );
    }
}

#[test]
fn unverified_rust_still_reports_no_checksums() {
    let dir = tempfile::tempdir().expect("The temporary directory must be created.");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("The lock file must be written.");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &ir(false, &catalog("1.99.1")),
        "ubuntu-26.04",
        &[],
        &catalog("1.99.1"),
    );
    let summary = outcome
        .recommendation
        .expect("The audit must report the verification gap.");
    assert!(summary.contains("rust@1.99.1(no_checksums)"), "{summary}");
}

#[test]
fn verified_uncatalogued_rust_blocks_once() {
    let dir = tempfile::tempdir().expect("The temporary directory must be created.");
    let catalog = catalog("1.84.1");
    let mut both = ir(true, &catalog);
    let second = both.jobs["plan"].clone();
    both.jobs.insert("other".to_owned(), second);
    let outcome =
        audit_prepare_installs_with_catalog(dir.path(), &both, "ubuntu-26.04", &[], &catalog);
    let hits: Vec<_> = outcome
        .blocking
        .iter()
        .filter(|line| line.starts_with(&format!("{NOT_CATALOGUED_CODE}:rust@1.84.1")))
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", outcome.blocking);
}

/// One plan job installing jactionlint, with or without `Verify jactionlint`.
fn jactionlint_ir(spec: &str, verified: bool) -> WorkflowIr {
    let mut workflow = ir(false, &ToolCatalog::pinned());
    let job = workflow.jobs.get_mut("plan").expect("plan job");
    job.steps = vec![step(
        PREPARE_PINNED_TOOLS_STEP,
        Some(StepRole::PreparePinnedTools),
        &["mise", "--no-config", "install", spec],
    )];
    if verified {
        job.steps.push(verifier_action(
            StepRole::VerifyJactionlint,
            velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
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
fn prepare_audit_accepts_the_production_inline_jactionlint_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned();
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, false);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(jactionlint_verifier(&catalog));
    let outcome =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    assert_eq!(outcome.recommendation, None);
}

#[test]
fn production_inline_jactionlint_step_does_not_hide_unknown_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Jactionlint, "9.9.9")
        .expect("exact");
    let workflow = ir_with_verifier(jactionlint_verifier(&catalog));
    let workflow = {
        let mut workflow = workflow;
        let job = workflow.jobs.get_mut("plan").expect("plan job");
        let prepare = job
            .steps
            .iter_mut()
            .find(|step| step.role == Some(StepRole::PreparePinnedTools))
            .expect("prepare step");
        let StepKind::Shell { run, .. } = &mut prepare.kind else {
            panic!("prepare step is a shell");
        };
        run[3] = "jactionlint@9.9.9".to_owned();
        workflow
    };
    let outcome =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(
        outcome
            .recommendation
            .as_deref()
            .is_some_and(|summary| summary.contains("jactionlint@9.9.9")),
        "unknown Jactionlint remains subject to lock coverage: {outcome:?}"
    );
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
