use std::collections::BTreeMap;

use super::{RUST_VERIFIER_USES, StepRole, validate_step_identity_scope};
use crate::workflow::{Step, StepKind};

fn action(uses: &str, with: BTreeMap<String, String>, env: BTreeMap<String, String>) -> Step {
    Step {
        name: "Verify Rust channel manifest".to_owned(),
        id: None,
        role: Some(StepRole::VerifyRustManifest),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env,
        },
    }
}

fn validate(step: Step) -> Result<(), crate::ContractError> {
    validate_step_identity_scope(&[step], "verifier-role-test")
}

#[test]
fn rust_verifier_role_accepts_only_the_two_empty_binding_actions() {
    for uses in RUST_VERIFIER_USES {
        validate(action(uses, BTreeMap::new(), BTreeMap::new()))
            .expect("fixed verifier action is a valid role payload");
    }

    // The existing inline shell role remains supported for callers that
    // verify in place, including release and upgrade workflows.
    let shell = Step {
        name: "Verify Rust channel manifest".to_owned(),
        id: None,
        role: Some(StepRole::VerifyRustManifest),
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    validate(shell).expect("previous shell payload shape remains valid");
}

#[test]
fn rust_verifier_role_rejects_path_mutations_and_unrelated_actions() {
    for uses in [
        "./.github/actions/velnor-verify-restored-rust/",
        "./.github/actions/velnor-verify-restored-rust@deadbeef",
        "./.github/actions/velnor-verify-restored-rust/../velnor-verify-rust-manifest",
        "./.github/actions/velnor-verify-restored-rust ",
        "./.github/actions/Velnor-verify-restored-rust",
        "./.github/actions/velnor-verify-rust-manifest-extra",
        "./.github/actions/velnor-verify-jactionlint",
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
    ] {
        assert!(
            validate(action(uses, BTreeMap::new(), BTreeMap::new())).is_err(),
            "unexpected accepted action path: {uses}"
        );
    }
}

#[test]
fn rust_verifier_role_rejects_even_empty_caller_inputs_and_environment() {
    let uses = RUST_VERIFIER_USES[1];
    for (key, value) in [
        ("phase", ""),
        ("rust-version", ""),
        ("RUSTUP_HOME", ""),
        ("CARGO_HOME", ""),
    ] {
        assert!(
            validate(action(
                uses,
                BTreeMap::from([(key.to_owned(), value.to_owned())]),
                BTreeMap::new(),
            ))
            .is_err(),
            "unexpected accepted action input: {key}"
        );
        assert!(
            validate(action(
                uses,
                BTreeMap::new(),
                BTreeMap::from([(key.to_owned(), value.to_owned())]),
            ))
            .is_err(),
            "unexpected accepted action environment: {key}"
        );
    }
}
