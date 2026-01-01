//! Pinned Mise setup emission: template shape plus strict insertion.
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::Step;
use velnor_actions_workflow_steps::{
    MiseSetup, RenderError, SETUP_MISE_NAME, checkout_step, mise_setup_step, plan_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn setup_step_shape_exact() -> Result<(), RenderError> {
    let step = mise_setup_step(&mise())?;
    assert_eq!(step.name, SETUP_MISE_NAME);
    let velnor_actions_contract_workflow::StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("setup must be an action step");
    };
    assert_eq!(uses, MISE_USES);
    assert_eq!(
        with,
        &BTreeMap::from([
            ("version".to_owned(), MISE_VERSION.to_owned()),
            ("sha256".to_owned(), MISE_SHA256.to_owned()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
        ])
    );
    Ok(())
}

#[test]
fn setup_pins_reject_every_shape_violation() {
    let bad_uses = [
        "jdx/mise-action@v4.3.0",
        "actions/checkout@c2a87611a18de5b3828c5652fe268e992400cb5c",
        "jdx/mise-action@short",
    ];
    for uses in bad_uses {
        let setup = MiseSetup {
            uses: uses.to_owned(),
            ..mise()
        };
        assert!(mise_setup_step(&setup).is_err(), "uses accepted: {uses}");
    }
    for version in ["", "latest", "v2026.9.16${{ x }}", "not a version!"] {
        let setup = MiseSetup {
            version: version.to_owned(),
            ..mise()
        };
        assert!(
            mise_setup_step(&setup).is_err(),
            "version accepted: {version}"
        );
    }
    for sha256 in ["", &"A".repeat(64), "abc123"] {
        let setup = MiseSetup {
            sha256: sha256.to_owned(),
            ..mise()
        };
        assert!(mise_setup_step(&setup).is_err(), "sha accepted: {sha256}");
    }
}

#[test]
fn strict_inserts_setup_before_mise_exec() -> Result<(), RenderError> {
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![lint]), &fixture_ctx())?;
    let setup_at = text.find(SETUP_MISE_NAME).expect("setup inserted");
    let mise_at = text.find("mise --no-config").expect("mise step kept");
    assert!(setup_at < mise_at, "setup must precede mise:\n{text}");
    for line in [
        format!("uses: {MISE_USES}"),
        format!("version: {MISE_VERSION}"),
        format!("sha256: {MISE_SHA256}"),
        "install: \"false\"".to_owned(),
        "env: \"false\"".to_owned(),
        "cache: ${{env.VELNOR_MISE_CACHE_ENABLED}}".to_owned(),
        "cache_save: \"false\"".to_owned(),
        "cache_key: mise-v2-hosted-".to_owned(),
    ] {
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(
        !text.contains("Restore Mise tools"),
        "P08: restores stay built-in:\n{text}"
    );
    for line in [
        "- name: Save Mise tools".to_owned(),
        "key: mise-v2-hosted-".to_owned(),
        "if: success() && github.event_name == 'push' && env.VELNOR_MISE_CACHE_ENABLED == 'true'"
            .to_owned(),
    ] {
        assert!(text.contains(&line), "sole owner saves {line}:\n{text}");
    }
    Ok(())
}

#[test]
fn strict_task_job_always_gets_setup() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let task = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run task",
                vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![plan, task]), &fixture_ctx())?;
    let task_at = text.find("velnor-task:").expect("task job");
    assert!(
        text[task_at..].contains(SETUP_MISE_NAME),
        "task setup:\n{text}"
    );
    Ok(())
}

#[test]
fn strict_keeps_single_wellformed_setup() -> Result<(), RenderError> {
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            mise_setup_step(&mise())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![lint]), &fixture_ctx())?;
    assert_eq!(text.matches(SETUP_MISE_NAME).count(), 1, "{text}");
    Ok(())
}

#[test]
fn strict_rejects_setup_misuse() -> Result<(), RenderError> {
    let lint_steps = || -> Result<Vec<Step>, RenderError> {
        Ok(vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ])
    };
    let render = |steps: Vec<Step>| {
        strict(
            &fixture_ir(vec![job("actionlint", "Actionlint", Vec::new(), steps)]),
            &fixture_ctx(),
        )
    };
    let mut dup = vec![checkout_step(&checkout_pin())?, mise_setup_step(&mise())?];
    dup.push(mise_setup_step(&mise())?);
    dup.extend(lint_steps()?[1..].to_vec());
    assert!(
        render(dup).is_err_and(|err| format!("{err:?}").contains("duplicate_setup_mise")),
        "duplicate setup must fail"
    );
    let mut misordered = lint_steps()?;
    misordered.push(mise_setup_step(&mise())?);
    assert!(
        render(misordered).is_err_and(|err| format!("{err:?}").contains("setup_mise_misordered")),
        "misordered setup must fail"
    );
    let malformed = velnor_actions_contract_workflow::Step {
        name: SETUP_MISE_NAME.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: velnor_actions_contract_workflow::StepKind::Action {
            uses: MISE_USES.to_owned(),
            with: BTreeMap::from([
                ("version".to_owned(), MISE_VERSION.to_owned()),
                ("install".to_owned(), "true".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    let mut steps = lint_steps()?;
    steps.insert(1, malformed);
    assert!(
        render(steps).is_err_and(|err| format!("{err:?}").contains("setup_mise_malformed")),
        "malformed setup must fail"
    );
    Ok(())
}

#[test]
fn strict_mixed_platform_checks_use_native_setup_and_keep_global_runner() -> Result<(), RenderError>
{
    use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};
    use velnor_actions_workflow_steps::setup::MISE_BINARY_SHA256_MACOS_ARM64;
    let mut check = job(
        "check-native",
        "Native",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Native task",
                mise_argv("node@22.0.0", "node", &["--version"]),
            )?,
        ],
    );
    check.1.runs_on = "macos-15".to_owned();
    check.1.check_runner = Some(CheckRunner {
        label: "macos-15".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::Hosted,
        container: None,
    });
    let ir = fixture_ir(vec![check.clone()]);
    let text = strict(&ir, &fixture_ctx())?;
    assert!(text.contains("runs-on: macos-15"));
    assert!(text.contains(MISE_BINARY_SHA256_MACOS_ARM64));
    assert!(text.contains("mise-v2-hosted-macos15-aarch64-apple-darwin-"));
    check.0 = "actionlint".to_owned();
    assert!(strict(&fixture_ir(vec![check.clone()]), &fixture_ctx()).is_err());
    check.0 = "check-native".to_owned();
    check.1.check_runner = None;
    assert!(strict(&fixture_ir(vec![check]), &fixture_ctx()).is_err());
    Ok(())
}

#[test]
fn qualified_linux_setup_cannot_bypass_macos_artifact_selection() -> Result<(), RenderError> {
    use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};
    use velnor_actions_workflow_cache::cache_p08::{mise_cache_key_for_tools, mise_setup_step_p08};
    let key = mise_cache_key_for_tools(
        "ubuntu26",
        "x86_64-unknown-linux-gnu",
        MISE_VERSION,
        &["node@22.0.0".to_owned()],
    )?;
    let mut check = job(
        "check-native",
        "Native",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            mise_setup_step_p08(&mise(), &key)?,
            scrubbed_shell_step(
                "Native task",
                mise_argv("node@22.0.0", "node", &["--version"]),
            )?,
        ],
    );
    check.1.runs_on = "macos-15".to_owned();
    check.1.check_runner = Some(CheckRunner {
        label: "macos-15".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::Hosted,
        container: None,
    });
    let error = strict(&fixture_ir(vec![check]), &fixture_ctx()).expect_err("wrong target pins");
    assert!(error.to_string().contains("setup_mise_pin_mismatch"));
    Ok(())
}

#[test]
fn public_ephemeral_ir_rejects_fork_admission_bypass() -> Result<(), RenderError> {
    use velnor_actions_contract_config::config::{
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
    use velnor_actions_contract_config::config::{CheckExecutor, CheckPlatform, CheckRunner};
    use velnor_actions_workflow_steps::setup::MISE_BINARY_SHA256_MACOS_ARM64;
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
    assert!(text.contains("mise-v2-hosted-macos15-aarch64-apple-darwin-"));
    assert!(!text.contains("rust@"));
    assert!(!text.contains("Prepare Rust components"));
    assert!(
        !step_names(&text, "auxiliary")
            .iter()
            .any(|name| name == SETUP_MISE_NAME)
    );
    Ok(())
}
