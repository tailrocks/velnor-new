use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::validate_step_sequence;
use velnor_actions_contract::{Step, StepKind, StepRole};

#[test]
fn jactionlint_verifier_role_accepts_only_shell_payloads() {
    let shell = Step {
        parallel: false,
        name: "Renamed verifier".to_owned(),
        id: None,
        role: Some(StepRole::VerifyJactionlint),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    validate_step_sequence(std::slice::from_ref(&shell), "verify")
        .expect("typed verifier role accepts a shell regardless of its name");

    let mut action = shell;
    action.kind = StepKind::Action {
        uses: "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        with: BTreeMap::new(),
        env: BTreeMap::new(),
    };
    assert!(
        validate_step_sequence(&[action], "verify")
            .expect_err("verifier role rejects action payload")
            .to_string()
            .contains("role_kind_mismatch")
    );
}
