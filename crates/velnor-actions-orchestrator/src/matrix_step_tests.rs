//! Obligation-step contract tests.
//!
//! Declared via `#[path]` from `matrix_step.rs` under `cfg(test)`.

use super::*;

/// Obligation fixture for step construction.
fn obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/rust/demo/clippy/default".to_owned(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["true".to_owned()],
    }
}

#[test]
fn identity_env_contract_enforces_in_every_build() {
    let task_id = "stack/rust/demo/clippy/default";
    let identity = obligation_identity_env(task_id, &format!("b3-{}", "a".repeat(64)), "id", "key");
    assert!(check_identity_env_contract(&identity, task_id).is_ok());
    let mut drifted = identity.clone();
    drifted.insert(
        TASK_ID_ENV.to_owned(),
        "stack/rust/demo/test/default".to_owned(),
    );
    let err = check_identity_env_contract(&drifted, task_id).expect_err("drift");
    assert!(
        err.to_string().contains("obligation_identity_mismatch"),
        "{err}"
    );
    let mut missing = identity.clone();
    missing.remove(TASK_ID_ENV);
    assert!(check_identity_env_contract(&missing, task_id).is_err());
    assert_eq!(OBLIGATION_TASK_ID_ENV, TASK_ID_ENV);
    // Log forging: a hostile task id renders as one truncated line.
    let forged = "stack/rust/demo/clippy/default\n::notice::spoofed";
    let err = check_identity_env_contract(&missing, forged).expect_err("forged");
    let text = err.to_string();
    assert!(!text.contains('\n'), "{text}");
    assert!(
        text.contains(
            "obligation_identity_mismatch:stack/rust/demo/clippy/default::notice::spoofed"
        ),
        "{text}"
    );
}

#[test]
fn obligation_step_carries_the_report_lookup_key() {
    let obligation = obligation();
    let step = obligation_step(&obligation, &ToolCatalog::pinned(), &[]).expect("step");
    let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind else {
        panic!("obligation must be a shell step");
    };
    assert_eq!(
        env.get(TASK_ID_ENV).map(String::as_str),
        Some(obligation.task_id.as_str())
    );
    assert!(run[2].contains(REPORT_OP), "wrapper reports: {run:?}");
}
