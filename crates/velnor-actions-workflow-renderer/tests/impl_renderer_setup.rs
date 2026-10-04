//! Pinned Mise setup emission: template shape plus strict insertion.
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind, ToolCacheDomain};
use velnor_actions_workflow_renderer::{
    MiseSetup, RenderError, SETUP_MISE_NAME, checkout_step, mise_setup_step, plan_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn setup_step_shape_exact() -> Result<(), RenderError> {
    let step = mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?;
    assert_eq!(step.name, SETUP_MISE_NAME);
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("setup must be an owner-bound helper");
    };
    let setup = mise();
    let expected = &setup.bootstraps[&(ToolCacheDomain::Full, LABEL.to_owned())];
    assert_eq!(invocation, expected.helper.invocation());
    assert!(invocation.installed_selectors().is_empty());
    assert_eq!(env, expected.helper.environment());
    assert_eq!(env.len(), 5);
    assert_eq!(env["VELNOR_MISE_TARGET"], "x86_64-unknown-linux-gnu");
    assert_eq!(env["MISE_DATA_DIR"], ToolCacheDomain::Full.root());
    assert_eq!(env["VELNOR_MISE_VERSION"], MISE_VERSION);
    assert_eq!(env["VELNOR_MISE_SHA256"], MISE_SHA256);
    assert_eq!(
        env["VELNOR_QUALIFIED_TOOL_IDENTITY"],
        format!("qualified-tools@{}", "a".repeat(64))
    );
    Ok(())
}

#[test]
fn setup_pins_reject_every_shape_violation() {
    let mut missing = mise();
    missing.bootstraps.clear();
    assert!(mise_setup_step(&missing, ToolCacheDomain::Full, LABEL).is_err());
    assert!(mise_setup_step(&mise(), ToolCacheDomain::Full, "ubuntu-latest").is_err());
    for version in ["", "latest", "v2026.9.16${{ x }}", "not a version!"] {
        let setup = MiseSetup {
            version: version.to_owned(),
            ..mise()
        };
        assert!(
            mise_setup_step(&setup, ToolCacheDomain::Full, LABEL).is_err(),
            "version accepted: {version}"
        );
    }
    for sha256 in ["", &"A".repeat(64), "abc123"] {
        let mut setup = mise();
        setup
            .bootstraps
            .get_mut(&(ToolCacheDomain::Full, LABEL.to_owned()))
            .expect("bootstrap fixture")
            .binary_sha256 = sha256.to_owned();
        assert!(
            mise_setup_step(&setup, ToolCacheDomain::Full, LABEL).is_err(),
            "sha accepted: {sha256}"
        );
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
    let restore_at = text.find("Restore Mise tools").expect("tools restored");
    let platform_at = text
        .find("Resolve tool cache platform")
        .expect("platform binding");
    assert!(
        platform_at < restore_at && restore_at < setup_at && setup_at < mise_at,
        "complete isolated state restored before installation/use:\n{text}"
    );
    assert!(text.contains("VELNOR_CACHE_IMAGE"));
    assert!(text.contains("missing runner image version"));
    assert!(text.contains("id: velnor-tools-cache"));
    for line in [
        format!("VELNOR_MISE_VERSION: {MISE_VERSION}"),
        format!("VELNOR_MISE_SHA256: {MISE_SHA256}"),
        "MISE_DATA_DIR:".to_owned(),
        "VELNOR_QUALIFIED_TOOL_IDENTITY:".to_owned(),
        "VELNOR_COMPILED_HELPER_SCHEMA:".to_owned(),
    ] {
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(
        text.contains("Restore Mise tools"),
        "explicit tools restore:\n{text}"
    );
    assert!(
        !text.contains("- name: Save Mise tools"),
        "consumer cannot export tools:\n{text}"
    );
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
            mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?,
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
    let mut dup = vec![
        checkout_step(&checkout_pin())?,
        mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?,
    ];
    dup.push(mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?);
    dup.extend(lint_steps()?[1..].to_vec());
    assert!(
        render(dup)
            .is_err_and(|err| format!("{err:?}").contains("mise_bootstrap_changed_or_duplicate")),
        "duplicate setup must fail"
    );
    let mut misordered = lint_steps()?;
    misordered.push(mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?);
    assert!(
        render(misordered)
            .is_err_and(|err| format!("{err:?}").contains("mise_bootstrap_after_tools")),
        "misordered setup must fail"
    );
    let mut malformed = mise_setup_step(&mise(), ToolCacheDomain::Full, LABEL)?;
    let StepKind::SourceBoundHelper { env, .. } = &mut malformed.kind else {
        panic!("bootstrap helper");
    };
    env.insert("VELNOR_MISE_SHA256".to_owned(), "b".repeat(64));
    let mut steps = lint_steps()?;
    steps.insert(1, malformed);
    assert!(
        render(steps)
            .is_err_and(|err| format!("{err:?}").contains("mise_bootstrap_changed_or_duplicate")),
        "malformed setup must fail"
    );
    Ok(())
}

#[test]
fn canonical_tool_restore_insertion_is_idempotent()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    let (_, mut demo) = job(
        "demo",
        "Demo",
        Vec::new(),
        vec![velnor_actions_workflow_renderer::shell_step(
            "Prepare pinned tools",
            vec![
                "mise".to_owned(),
                "--no-config".to_owned(),
                "install".to_owned(),
                "rust@1.98.1".to_owned(),
            ],
            BTreeMap::new(),
        )?],
    );
    velnor_actions_workflow_renderer::cache_p08::ensure_setup_p08(
        "demo",
        &mut demo,
        &mise(),
        false,
        "x86_64-unknown-linux-gnu",
        &[],
    )?;
    let once = demo.clone();
    velnor_actions_workflow_renderer::cache_p08::ensure_setup_p08(
        "demo",
        &mut demo,
        &mise(),
        false,
        "x86_64-unknown-linux-gnu",
        &[],
    )?;
    assert_eq!(
        demo.steps, once.steps,
        "repeat rendering retains one platform binding and restore"
    );
    assert_eq!(
        demo.steps
            .iter()
            .filter(|step| step.name == "Restore Mise tools")
            .count(),
        1
    );
    assert_eq!(
        demo.steps
            .iter()
            .filter(|step| step.name == "Resolve tool cache platform")
            .count(),
        1
    );
    Ok(())
}
