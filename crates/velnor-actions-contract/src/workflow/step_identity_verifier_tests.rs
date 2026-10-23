use std::collections::BTreeMap;

use super::{
    JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES, StepRole, validate_step_identity_scope,
};
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

fn jactionlint_action(
    uses: &str,
    with: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
) -> Step {
    Step {
        name: "neutral presentation".to_owned(),
        id: None,
        role: Some(StepRole::VerifyJactionlint),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env,
        },
    }
}

fn verifier_action(uses: &str, role: Option<StepRole>) -> Step {
    let mut step = jactionlint_action(uses, BTreeMap::new(), BTreeMap::new());
    step.role = role;
    step
}

fn validate(step: Step) -> Result<(), crate::ContractError> {
    validate_step_identity_scope(&[step], "verifier-role-test")
}

#[test]
fn rust_verifier_role_accepts_only_the_two_empty_binding_actions() {
    for uses in RUST_VERIFIER_USES {
        validate(action(uses, BTreeMap::new(), BTreeMap::new()))
            .expect("fixed verifier action is a valid role payload");

        let mut renamed = action(uses, BTreeMap::new(), BTreeMap::new());
        renamed.name = "neutral verifier label".to_owned();
        validate(renamed).expect("verifier role does not depend on the display name");
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
        ("phase", "untrusted"),
        ("CARGO_HOME", "/tmp/untrusted-cargo"),
    ] {
        assert!(
            validate(action(
                uses,
                BTreeMap::from([(key.to_owned(), value.to_owned())]),
                BTreeMap::new()
            ))
            .is_err(),
            "unexpected accepted action input: {key}"
        );
        assert!(
            validate(action(
                uses,
                BTreeMap::new(),
                BTreeMap::from([(key.to_owned(), value.to_owned())])
            ))
            .is_err(),
            "unexpected accepted action environment: {key}"
        );
    }
}

#[test]
fn jactionlint_verifier_role_accepts_only_its_exact_empty_binding_action() {
    validate(jactionlint_action(
        JACTIONLINT_VERIFIER_USES,
        BTreeMap::new(),
        BTreeMap::new(),
    ))
    .expect("exact local verifier action is a valid role payload");

    for uses in [
        "./.github/actions/velnor-verify-jactionlint/",
        "./.github/actions/velnor-verify-jactionlint@deadbeef",
        "./.github/actions/velnor-verify-jactionlint/../velnor-verify-restored-rust",
        "./.github/actions/velnor-verify-jactionlint ",
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
    ] {
        assert!(
            validate(jactionlint_action(uses, BTreeMap::new(), BTreeMap::new())).is_err(),
            "unexpected accepted action path: {uses}"
        );
    }

    for (key, value) in [
        ("tool", ""),
        ("MISE_DATA_DIR", ""),
        ("tool", "untrusted"),
        ("MISE_DATA_DIR", "/tmp/selected"),
    ] {
        assert!(
            validate(jactionlint_action(
                JACTIONLINT_VERIFIER_USES,
                BTreeMap::from([(key.to_owned(), value.to_owned())]),
                BTreeMap::new(),
            ))
            .is_err(),
            "unexpected accepted action input: {key}"
        );
        assert!(
            validate(jactionlint_action(
                JACTIONLINT_VERIFIER_USES,
                BTreeMap::new(),
                BTreeMap::from([(key.to_owned(), value.to_owned())]),
            ))
            .is_err(),
            "unexpected accepted action environment: {key}"
        );
    }
}

#[test]
fn exact_verifier_action_refs_require_their_matching_role() {
    for uses in RUST_VERIFIER_USES {
        let error = validate(verifier_action(uses, None))
            .expect_err("roleless exact Rust verifier ref must fail at contract boundary");
        assert!(
            error.to_string().contains("verifier_action_role_mismatch"),
            "{error}"
        );
    }

    let roleless_jactionlint = validate(verifier_action(JACTIONLINT_VERIFIER_USES, None))
        .expect_err("roleless exact Jactionlint verifier ref must fail at contract boundary");
    assert!(
        roleless_jactionlint
            .to_string()
            .contains("verifier_action_role_mismatch"),
        "{roleless_jactionlint}"
    );
    let mut roleless_canonical_name = verifier_action(JACTIONLINT_VERIFIER_USES, None);
    roleless_canonical_name.name = "Verify jactionlint".to_owned();
    assert!(
        validate(roleless_canonical_name)
            .expect_err("canonical display name cannot replace the required role")
            .to_string()
            .contains("verifier_action_role_mismatch")
    );

    for (uses, role) in [
        (RUST_VERIFIER_USES[1], StepRole::VerifyJactionlint),
        (JACTIONLINT_VERIFIER_USES, StepRole::VerifyRustManifest),
    ] {
        let error = validate(verifier_action(uses, Some(role)))
            .expect_err("crossed verifier ref and role must fail closed");
        assert!(
            error.to_string().contains("verifier_action_role_mismatch"),
            "{error}"
        );
    }
}
