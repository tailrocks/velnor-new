use std::collections::BTreeMap;

use super::{ToolCatalog, audit_prepare_installs_with_catalog, ir};
use velnor_actions_contract::{Step, StepKind, StepRole};

fn catalog(rust: &str) -> ToolCatalog {
    ToolCatalog::pinned()
        .with_project_tool(velnor_actions_mise::PinnedTool::Rust, rust)
        .expect("exact Rust catalog entry")
}

const LOCK: &str =
    "lockfile_version = 3\n\n[[tools.rust]]\nversion = \"1.99.1\"\nbackend = \"core:rust\"\n";

#[test]
fn typed_action_calls_reject_unapproved_execution_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    for mutation in [
        ActionMutation::Role,
        ActionMutation::Id,
        ActionMutation::Condition,
        ActionMutation::Parallel,
        ActionMutation::Input,
        ActionMutation::Environment,
    ] {
        let mut workflow = ir(true);
        let installed = workflow
            .jobs
            .get_mut("plan")
            .expect("plan job")
            .steps
            .last_mut()
            .expect("installed verifier");
        mutate_action(installed, mutation);

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
            "{mutation:?} must invalidate the typed verifier: {:?}",
            outcome.blocking
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum ActionMutation {
    Role,
    Id,
    Condition,
    Parallel,
    Input,
    Environment,
}

fn restored_shell(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        &catalog.for_emission(),
        crate::workflow::ToolchainPhase::Restored,
    )
    .expect("production restored Rust shell verifier")
}

fn installed_shell(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        &catalog.for_emission(),
        crate::workflow::ToolchainPhase::Installed,
    )
    .expect("production installed Rust shell verifier")
}

fn jactionlint_shell(catalog: &ToolCatalog) -> Step {
    crate::workflow::workflow_policy::jactionlint_verify_step::verifier_shell_step(
        &catalog.for_emission(),
    )
    .expect("production Jactionlint shell verifier")
}

fn mutate_action(step: &mut Step, mutation: ActionMutation) {
    match mutation {
        ActionMutation::Role => step.role = None,
        ActionMutation::Id => step.id = Some(velnor_actions_contract::StepId::CacheKeys),
        ActionMutation::Condition => step.condition = Some("false".to_owned()),
        ActionMutation::Parallel => step.parallel = true,
        ActionMutation::Input => {
            if let StepKind::Action { with, .. } = &mut step.kind {
                with.insert("unexpected".to_owned(), "value".to_owned());
            }
        }
        ActionMutation::Environment => {
            if let StepKind::Action { env, .. } = &mut step.kind {
                env.insert("RUSTUP_HOME".to_owned(), String::new());
            }
        }
    }
}

#[test]
fn canonical_inline_verifiers_accept_neutral_display_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let catalog = catalog("1.99.1");
    let mut workflow = ir(false);
    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    let mut restored = restored_shell(&catalog);
    restored.name = "Restore integrity evidence".to_owned();
    let mut installed = installed_shell(&catalog);
    installed.name = "Check installed toolchain bytes".to_owned();
    steps.insert(0, restored);
    steps.push(installed);

    let outcome =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(outcome.blocking.is_empty(), "{:?}", outcome.blocking);
    assert_eq!(outcome.recommendation, None);
}

#[test]
fn duplicate_installed_calls_across_inline_and_action_forms_are_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = catalog("1.99.1");
    let mut workflow = ir(true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(installed_shell(&catalog));

    let outcome =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("duplicate_verifier_call:plan:installed-rust")),
        "typed plus inline installed calls must count as duplicates: {:?}",
        outcome.blocking
    );
}

#[test]
fn installed_rust_verification_must_follow_all_component_setup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut workflow = ir(true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(Step {
            parallel: false,
            name: "Additional Rust components".to_owned(),
            id: None,
            role: Some(StepRole::PrepareRustComponents),
            condition: None,
            kind: StepKind::Shell {
                run: vec![
                    "rustup".to_owned(),
                    "component".to_owned(),
                    "add".to_owned(),
                    "clippy".to_owned(),
                ],
                env: BTreeMap::new(),
            },
        });

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
        "installed verification before component setup must not attest: {:?}",
        outcome.blocking
    );
}

#[test]
fn installed_rust_verification_immediately_follows_final_setup_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let catalog = catalog("1.99.1");
    let mut workflow = ir(false);
    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    let StepKind::Shell { run, .. } = &mut steps[0].kind else {
        panic!("prepare is emitted as a shell step");
    };
    run.push(format!(
        "jactionlint@{}",
        velnor_actions_mise::JACTIONLINT_VERSION
    ));
    steps.insert(0, restored_shell(&catalog));
    steps.push(jactionlint_shell(&catalog));
    steps.push(Step {
        parallel: false,
        name: "Install Rust components".to_owned(),
        id: None,
        role: Some(StepRole::PrepareRustComponents),
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "rustup".to_owned(),
                "component".to_owned(),
                "add".to_owned(),
            ],
            env: BTreeMap::new(),
        },
    });
    steps.push(installed_shell(&catalog));

    let valid =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(valid.blocking.is_empty(), "{:?}", valid.blocking);

    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    let installed_at = steps.len() - 1;
    steps.insert(
        installed_at,
        Step {
            parallel: false,
            name: "Unrelated step".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), "between".to_owned()],
                env: BTreeMap::new(),
            },
        },
    );

    let invalid =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(
        invalid
            .blocking
            .iter()
            .any(|message| message.starts_with("rust_verifier_sequence_invalid:plan:")),
        "an intervening step must invalidate installed evidence: {:?}",
        invalid.blocking
    );
}

#[test]
fn installed_rust_follows_jactionlint_when_no_components_are_needed() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.lock"), LOCK).expect("lock");
    let catalog = catalog("1.99.1");
    let mut workflow = ir(false);
    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    let StepKind::Shell { run, .. } = &mut steps[0].kind else {
        panic!("prepare is emitted as a shell step");
    };
    run.push(format!(
        "jactionlint@{}",
        velnor_actions_mise::JACTIONLINT_VERSION
    ));
    steps.insert(0, restored_shell(&catalog));
    steps.push(jactionlint_shell(&catalog));
    steps.push(installed_shell(&catalog));

    let valid =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(valid.blocking.is_empty(), "{:?}", valid.blocking);

    let steps = &mut workflow.jobs.get_mut("plan").expect("plan job").steps;
    let installed_at = steps.len() - 1;
    steps.insert(
        installed_at,
        Step {
            parallel: false,
            name: "Unrelated step".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), "between".to_owned()],
                env: BTreeMap::new(),
            },
        },
    );

    let invalid =
        audit_prepare_installs_with_catalog(dir.path(), &workflow, "ubuntu-26.04", &[], &catalog);
    assert!(
        invalid
            .blocking
            .iter()
            .any(|message| message.starts_with("rust_verifier_sequence_invalid:plan:")),
        "an intervening step must invalidate installed evidence: {:?}",
        invalid.blocking
    );
}
