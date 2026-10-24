use super::*;

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
fn strict_scale_set_mode_rejects_typed_darwin_ephemeral_check() -> Result<(), RenderError> {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
        SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, ValidatedImageReference,
    };
    use velnor_actions_contract::{ExecutionMode, WorkflowPolicy};
    use velnor_actions_workflow_renderer::workflow_routing::WorkflowRoutingPolicy;

    let mut check = job(
        "check-external",
        "Check / external",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    check.1.runs_on = "native-scale-set".to_owned();
    check.1.condition = Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
    check.1.check_runner = Some(CheckRunner {
        label: "native-scale-set".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    });
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    let image = ValidatedImageReference::parse(
        "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_test_image".to_owned()))?;
    let routing = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector, Some(image));
    let result =
        velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared_with_routing(
            &fixture_ir(vec![check]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
            &mise(),
            &routing,
        );
    let Err(error) = result else {
        return Err(RenderError::InvalidWorkflow(
            "scale_set_mode_accepted_native_check".to_owned(),
        ));
    };
    assert!(
        error
            .to_string()
            .contains("typed_ephemeral_check_policy_invalid")
    );
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
