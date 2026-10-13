#[test]
fn public_ephemeral_ir_rejects_fork_admission_bypass() -> Result<(), RenderError> {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
    };
    let mut check = job(
        "check-external",
        "Check / external",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Native task",
                mise_argv("node@22.0.0", "node", &["--version"]),
            )?,
        ],
    );
    check.1.runs_on = "native-scale-set".to_owned();
    check.1.check_runner = Some(CheckRunner {
        label: "native-scale-set".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    });
    for condition in [
        None,
        Some("true"),
        Some("always()"),
        Some("success()"),
        Some("github.event_name == 'pull_request'"),
    ] {
        check.1.condition = condition.map(str::to_owned);
        let error = strict(&fixture_ir(vec![check.clone()]), &fixture_ctx())
            .expect_err("unguarded external runner cannot render");
        assert!(
            error
                .to_string()
                .contains("ephemeral_check_requires_admission_condition")
        );
    }
    check.1.condition = Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
    let text = strict(&fixture_ir(vec![check]), &fixture_ctx())?;
    assert!(text.contains(EPHEMERAL_CHECK_ADMISSION_CONDITION));
    assert!(text.contains("runs-on: native-scale-set"));
    Ok(())
}

#[test]
fn native_check_without_catalog_tools_still_bootstraps_mise() -> Result<(), RenderError> {
    use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner};
    use velnor_actions_workflow_renderer::setup::MISE_BINARY_SHA256_MACOS_ARM64;
    let plain_step = || scrubbed_shell_step("Native helper", vec!["true".to_owned()]);
    let mut check = job(
        "check-native",
        "Check / native",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plain_step()?],
    );
    check.1.runs_on = "macos-15".to_owned();
    check.1.check_runner = Some(CheckRunner {
        label: "macos-15".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::Hosted,
        container: None,
    });
    let auxiliary = job(
        "auxiliary",
        "Auxiliary",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plain_step()?],
    );
    let text = strict(&fixture_ir(vec![check, auxiliary]), &fixture_ctx())?;
    let names = step_names(&text, "check-native");
    assert!(names.iter().any(|name| name == SETUP_MISE_NAME));
    assert!(text.contains(MISE_BINARY_SHA256_MACOS_ARM64));
    assert!(!text.contains("mise-v1-"));
    assert!(!text.contains("Swatinem/rust-cache"));
    assert!(!text.contains("rust@"));
    assert!(!text.contains("Prepare Rust components"));
    assert!(
        !step_names(&text, "auxiliary")
            .iter()
            .any(|name| name == SETUP_MISE_NAME)
    );
    Ok(())
}
