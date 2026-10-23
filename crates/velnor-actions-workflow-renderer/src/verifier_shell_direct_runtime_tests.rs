use super::*;

#[test]
fn release_verifier_command_uses_the_same_startup_safe_boundary() {
    let script = crate::selected_mise_verifier::test_script("2.3.4");
    let step = crate::verifier_shell::render_command(
        "Verify jactionlint",
        &["sh".to_owned(), "-c".to_owned(), script],
    )
    .expect("release verifier renders");
    assert_emitted_boundary(&step);
}

#[test]
fn direct_release_verifiers_match_the_typed_renderer_for_each_role() {
    let rust_body = "test \"$value\" = \"${VELNOR_TOOL_RUST}\"";
    let rust_argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "{} {rust_body}",
            crate::toolchain_env::credential_unset_prelude()
        ),
    ];
    let direct_rust =
        crate::verifier_shell::render_command(crate::VERIFY_RUST_TOOLCHAIN_STEP, &rust_argv)
            .expect("canonical direct Rust command");
    let typed_rust = rendered_probe(StepRole::VerifyRustManifest, rust_body);
    assert_eq!(direct_rust, typed_rust);
    assert_emitted_boundary(&direct_rust);

    for version in ["2.3.4", "${VELNOR_TOOL_JACTIONLINT}"] {
        let script = crate::selected_mise_verifier::test_script(version);
        let direct_jactionlint = crate::verifier_shell::render_command(
            "Verify jactionlint",
            &["sh".to_owned(), "-c".to_owned(), script.clone()],
        )
        .expect("canonical direct Jactionlint command");
        let typed_jactionlint = rendered_probe(StepRole::VerifyJactionlint, &script);
        assert_eq!(direct_jactionlint, typed_jactionlint);
        assert_emitted_boundary(&direct_jactionlint);
    }
}

#[test]
fn direct_release_verifier_rejects_wrong_shape_and_altered_payloads() {
    let unset = crate::toolchain_env::credential_unset_prelude();
    assert!(
        crate::verifier_shell::render_command(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            &["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
        )
        .is_err()
    );
    assert!(
        crate::verifier_shell::render_command(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            &["bash".to_owned(), "-c".to_owned(), format!("{unset} true")],
        )
        .is_err()
    );

    let mut altered_jactionlint = crate::selected_mise_verifier::test_script("2.3.4");
    altered_jactionlint.push(';');
    assert!(
        crate::verifier_shell::render_command(
            "Verify jactionlint",
            &["sh".to_owned(), "-c".to_owned(), altered_jactionlint],
        )
        .is_err()
    );
}
