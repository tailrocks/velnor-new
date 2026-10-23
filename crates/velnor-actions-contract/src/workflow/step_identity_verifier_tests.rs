use std::collections::BTreeMap;

use super::super::{
    JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES, StepRole, validate_step_identity_scope,
};
use crate::workflow::{Step, StepKind};

fn verifier(uses: &str, role: Option<StepRole>) -> Step {
    Step {
        name: "Neutral verifier presentation".to_owned(),
        id: None,
        role,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn valid(step: Step) -> Result<(), crate::ContractError> {
    validate_step_identity_scope(&[step], "verifier-role-test")
}

#[test]
fn verifier_action_refs_are_bound_to_their_semantic_roles() {
    for uses in RUST_VERIFIER_USES {
        valid(verifier(uses, Some(StepRole::VerifyRustManifest)))
            .expect("exact Rust action accepts its role with a neutral name");
        assert!(
            valid(verifier(uses, None))
                .expect_err("roleless Rust verifier ref")
                .to_string()
                .contains("verifier_action_role_mismatch")
        );
        assert!(valid(verifier(uses, Some(StepRole::VerifyJactionlint))).is_err());
    }
    valid(verifier(
        JACTIONLINT_VERIFIER_USES,
        Some(StepRole::VerifyJactionlint),
    ))
    .expect("exact jactionlint action accepts its role");
    assert!(valid(verifier(JACTIONLINT_VERIFIER_USES, None)).is_err());
    assert!(
        valid(verifier(
            JACTIONLINT_VERIFIER_USES,
            Some(StepRole::VerifyRustManifest),
        ))
        .is_err()
    );
}

#[test]
fn verifier_actions_reject_bindings_and_metadata() {
    let base = verifier(RUST_VERIFIER_USES[1], Some(StepRole::VerifyRustManifest));
    let uses = match &base.kind {
        StepKind::Action { uses, .. } => uses.clone(),
        _ => unreachable!("verifier helper creates an action"),
    };
    for mutate in ["with", "env", "id", "condition", "parallel"] {
        let mut changed = base.clone();
        match mutate {
            "with" => {
                let StepKind::Action { with, .. } = &mut changed.kind else {
                    unreachable!()
                };
                with.insert("phase".to_owned(), "restored".to_owned());
            }
            "env" => {
                let StepKind::Action { env, .. } = &mut changed.kind else {
                    unreachable!()
                };
                env.insert("RUSTUP_HOME".to_owned(), "/tmp/rustup".to_owned());
            }
            "id" => changed.id = Some(crate::workflow::StepId::Plan),
            "condition" => changed.condition = Some("always()".to_owned()),
            "parallel" => changed.parallel = true,
            _ => unreachable!(),
        }
        assert!(valid(changed).is_err(), "accepted changed {mutate}");
    }

    let mut unknown = base;
    let StepKind::Action { uses: action, .. } = &mut unknown.kind else {
        unreachable!()
    };
    *action = format!("{uses}/extra");
    assert!(
        valid(unknown).is_err(),
        "accepted near-match verifier action"
    );
}

#[test]
fn verifier_roles_continue_to_accept_inline_shell_payloads() {
    for role in [StepRole::VerifyRustManifest, StepRole::VerifyJactionlint] {
        let step = Step {
            name: "Renamed inline verifier".to_owned(),
            id: None,
            role: Some(role),
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
                env: BTreeMap::new(),
            },
        };
        valid(step).expect("existing typed shell role remains accepted");
    }
}
