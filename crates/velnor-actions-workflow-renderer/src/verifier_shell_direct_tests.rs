use std::collections::BTreeMap;

use velnor_actions_contract::StepRole;

use super::runtime::assert_emitted_boundary;

#[test]
fn rust_verifier_command_uses_the_same_typed_startup_boundary() {
    let argv = [
        "sh".to_owned(),
        "-c".to_owned(),
        "set -eu; printf '%s\\n' rust-verifier-ok".to_owned(),
    ];
    let rendered = crate::verifier_shell::render_command(
        StepRole::VerifyRustManifest,
        "Neutral verifier label",
        &argv,
    )
    .expect("Rust verifier command renders");
    let mut canonical_step =
        crate::steps::shell_step("Neutral verifier label", argv.to_vec(), BTreeMap::new())
            .expect("canonical scrubbed shell step");
    canonical_step.role = Some(StepRole::VerifyRustManifest);
    let canonical =
        crate::verifier_composite::render_verifier_workflow_step(&canonical_step, &BTreeMap::new())
            .expect("canonical typed verifier rendering");
    assert_eq!(rendered, canonical);
    assert_emitted_boundary(&rendered);
}

#[test]
fn rust_verifier_command_rejects_a_preembedded_credential_prelude() {
    let prelude = crate::toolchain_env::credential_unset_prelude();
    let error = crate::verifier_shell::render_command(
        StepRole::VerifyRustManifest,
        "Neutral verifier label",
        &[
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{prelude} set -eu"),
        ],
    )
    .expect_err("the typed shell constructor adds the one canonical prelude");
    assert!(matches!(
        error,
        crate::RenderError::InvalidWorkflow(problem)
            if problem == "verifier_shell_script_duplicate_credential_prelude"
    ));
}
