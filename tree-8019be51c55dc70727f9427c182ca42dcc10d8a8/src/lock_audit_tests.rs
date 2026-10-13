//! Install-audit glue tests: extraction failures block, labels gate loudly.
use std::collections::BTreeMap;

use velnor_actions_contract::workflow::validator_kind::ValidatorKind;
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
};
use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
use velnor_actions_workflow_renderer::render::ValidatorCommand;
use velnor_actions_workflow_renderer::steps::DENY_STEP_NAME;

use super::audit_prepare_installs;
use crate::vectors::CARGO_DENY_VERSION;

fn shell_job(run: Vec<String>) -> Job {
    Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
            condition: None,
            kind: StepKind::Shell {
                run,
                env: BTreeMap::new(),
            },
        }],
    }
}

/// Deny privilege-drop command shape: `sh -c` over install plus payload.
fn deny_command(script: &str) -> ValidatorCommand {
    ValidatorCommand {
        validator: ValidatorKind::CargoDeny,
        name: DENY_STEP_NAME.to_owned(),
        argv: vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
    }
}

fn ir_for(job: Job) -> WorkflowIr {
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
    }
}

#[test]
fn foreign_spec_and_malformed_argv_block() {
    let dir = tempfile::tempdir().expect("tempdir");
    let foreign = ir_for(shell_job(vec![
        "mise".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
        "node@20.0.0".to_owned(),
    ]));
    let outcome = audit_prepare_installs(dir.path(), &foreign, "ubuntu-26.04", &[]);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("unauditable_install_spec:node@20.0.0")),
        "foreign spec must block: {:?}",
        outcome.blocking
    );
    let shapeless = ir_for(shell_job(vec!["mise".to_owned(), "frobnicate".to_owned()]));
    let outcome = audit_prepare_installs(dir.path(), &shapeless, "ubuntu-26.04", &[]);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("unauditable_prepare_argv:plan")),
        "shapeless argv must block: {:?}",
        outcome.blocking
    );
}

#[test]
fn unknown_label_skips_loudly_without_blocking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = ir_for(shell_job(vec![
        "mise".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
        "rust@1.98.1".to_owned(),
    ]));
    let outcome = audit_prepare_installs(dir.path(), &ir, "self-hosted-1", &[]);
    assert!(outcome.blocking.is_empty());
    let summary = outcome.recommendation.expect("loud skip");
    assert!(summary.contains("self-hosted-1"), "{summary}");
    assert!(summary.contains("tool_install_unverified"), "{summary}");
}

#[test]
fn empty_install_set_stays_silent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ir = ir_for(shell_job(vec!["true".to_owned()]));
    ir.jobs.get_mut("plan").expect("plan").steps.clear();
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &[]);
    assert!(outcome.blocking.is_empty());
    assert!(outcome.recommendation.is_none());
}

/// Well-formed deny command with the pinned spec: audited, not silent.
///
/// Validator steps materialize at render time, so the audit reads the
/// typed command, not the IR. No lock exists in the temp root, so the
/// deny subject lands in the missing-lock advisory by name instead of
/// passing silently.
#[test]
fn deny_install_is_audited_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = format!(
        "{{ mise --no-env --no-hooks install cargo-deny@{CARGO_DENY_VERSION} && unset FOO }} && payload"
    );
    let mut ir = ir_for(shell_job(vec!["true".to_owned()]));
    ir.jobs.get_mut("plan").expect("plan").steps.clear();
    let commands = vec![deny_command(&script)];
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    let summary = outcome.recommendation.expect("deny advisory");
    assert!(
        summary.contains(&format!("cargo-deny@{CARGO_DENY_VERSION}")),
        "{summary}"
    );
}

/// Deny command drift fails closed: a foreign spec and a drifted
/// version block instead of auditing the wrong set, while an
/// isolated `exec` command contributes nothing.
#[test]
fn deny_drift_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ir = ir_for(shell_job(vec!["true".to_owned()]));
    ir.jobs.get_mut("plan").expect("plan").steps.clear();
    for script in [
        "{ mise --no-env --no-hooks install node@20.0.0 && unset FOO } && payload".to_owned(),
        "{ mise --no-env --no-hooks install cargo-deny@9.9.9 && unset FOO } && payload".to_owned(),
    ] {
        let commands = vec![deny_command(&script)];
        let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
        assert!(
            !outcome.blocking.is_empty(),
            "drifted deny command must block: {script}"
        );
    }
    // An isolated exec command is not an install vector: skipped.
    let commands = vec![deny_command(
        "mise exec cargo-deny@0.20.2 -- cargo deny check",
    )];
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
    assert!(outcome.blocking.is_empty());
    assert!(outcome.recommendation.is_none());
    // A catalog spec inside a validator command resolves through
    // nothing: commands audit validator pins only, never catalog tools.
    let commands = vec![deny_command(
        "{ mise --no-env --no-hooks install rust@1.98.1 && unset FOO } && payload",
    )];
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("unauditable_install_spec:rust@1.98.1")),
        "misplaced catalog spec must block: {:?}",
        outcome.blocking
    );
    // A `cargo install` payload is not a mise vector: skipped, since
    // the extractor anchors on `mise` followed by flags plus `install`.
    let commands = vec![ValidatorCommand {
        validator: ValidatorKind::CargoDeny,
        name: DENY_STEP_NAME.to_owned(),
        argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "cargo install foo && mise --no-env exec rust@1.98.1 -- cargo build".to_owned(),
        ],
    }];
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
    assert!(outcome.blocking.is_empty());
    assert!(outcome.recommendation.is_none());
}

/// A bare `install` (zero specs) blocks: versions would come from
/// `mise.toml`, so silence would be a fail-open.
#[test]
fn bare_install_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = ir_for(shell_job(vec![
        "mise".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
    ]));
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &[]);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("bare_install_step:plan")),
        "bare install must block: {:?}",
        outcome.blocking
    );
}

/// A catalog spec with a drifted version is unauditable: the spec
/// version must equal the pin exactly, never silently re-pinned.
#[test]
fn drifted_spec_version_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ir = ir_for(shell_job(vec![
        "mise".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
        "rust@1.97.0".to_owned(),
    ]));
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &[]);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("unauditable_install_spec:rust@1.97.0")),
        "drifted spec must block: {:?}",
        outcome.blocking
    );
}

/// An unclassifiable `mise` vector in a validator command blocks: it
/// is neither an `install` nor an isolated `exec`, so silence would
/// be a fail-open.
#[test]
fn unclassifiable_validator_vector_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ir = ir_for(shell_job(vec!["true".to_owned()]));
    ir.jobs.get_mut("plan").expect("plan").steps.clear();
    for script in [
        "mise frobnicate cargo-deny@0.20.2",
        "mise --no-env --no-hooks",
    ] {
        let commands = vec![deny_command(script)];
        let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &commands);
        assert!(
            outcome
                .blocking
                .iter()
                .any(|line| line.contains("unauditable_validator_argv")),
            "unclassifiable vector must block: {script} -> {:?}",
            outcome.blocking
        );
    }
}

/// Oversized tool files block instead of loading unbounded.
#[test]
fn oversized_tool_file_blocks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let big = "x".repeat(1024 * 1024 + 1);
    std::fs::write(dir.path().join("mise.lock"), &big).expect("write lock");
    let ir = ir_for(shell_job(vec![
        "mise".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
        "rust@1.98.1".to_owned(),
    ]));
    let outcome = audit_prepare_installs(dir.path(), &ir, "ubuntu-26.04", &[]);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|line| line.contains("oversized_tool_file:mise.lock")),
        "oversized lock must block: {:?}",
        outcome.blocking
    );
}
