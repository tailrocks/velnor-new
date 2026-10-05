//! Pinned Mise setup emission: template shape plus strict insertion.
use std::collections::BTreeMap;
use velnor_actions_contract::Step;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_workflow_renderer::steps::{
    TOOLS_CACHE_RESTORE_CONDITION, TOOLS_CACHE_SAVE_CONDITION,
};
use velnor_actions_workflow_renderer::{
    MiseSetup, RenderError, SETUP_MISE_NAME, checkout_step, mise_setup_step, plan_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn setup_step_shape_exact() -> Result<(), RenderError> {
    let step = mise_setup_step(&mise())?;
    assert_eq!(step.name, SETUP_MISE_NAME);
    let velnor_actions_contract::StepKind::Action { uses, with, env } = &step.kind else {
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
    assert_eq!(
        env,
        &BTreeMap::from([
            (
                "MISE_DATA_DIR".to_owned(),
                "${{ runner.temp }}/velnor/mise-bootstrap".to_owned()
            ),
            ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
            ("MISE_NO_ENV".to_owned(), "1".to_owned()),
            ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
            ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
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
    let image_at = text
        .find("Resolve tool-cache image")
        .expect("runner image probe inserted");
    let restore_at = text
        .find("Restore Mise tools")
        .expect("explicit restore inserted");
    let guard_at = text
        .find("Verify restored Mise bootstrap")
        .expect("restored bootstrap guard inserted");
    assert!(
        image_at < restore_at && restore_at < guard_at && guard_at < setup_at,
        "image probe, restore, verification, and executable setup order:\n{text}"
    );
    for line in [
        format!("uses: {MISE_USES}"),
        format!("version: {MISE_VERSION}"),
        format!("sha256: {MISE_SHA256}"),
        "install: \"false\"".to_owned(),
        "env: \"false\"".to_owned(),
        "cache: \"false\"".to_owned(),
        "cache_save: \"false\"".to_owned(),
        "MISE_DATA_DIR: ${{ runner.temp }}/velnor/mise-bootstrap".to_owned(),
    ] {
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(text.contains(&format!("if: {TOOLS_CACHE_RESTORE_CONDITION}")));
    assert!(text.contains(&format!(
        "if: {CACHE_SAVE_CONDITION} && {TOOLS_CACHE_SAVE_CONDITION}"
    )));
    for line in [
        "- name: Save Mise tools".to_owned(),
        "key: mise-v3-${{ runner.os }}-${{ runner.arch }}-${{ env.VELNOR_CACHE_IMAGE_OS }}-${{ env.VELNOR_CACHE_IMAGE_VERSION }}-".to_owned(),
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
    let image_at = text
        .find("Resolve tool-cache image")
        .expect("legacy setup gets cache gate");
    let restore_at = text
        .find("Restore Mise tools")
        .expect("legacy setup gets explicit restore");
    let guard_at = text
        .find("Verify restored Mise bootstrap")
        .expect("legacy setup gets bootstrap verification");
    let setup_at = text.find(SETUP_MISE_NAME).expect("setup remains singular");
    assert!(
        image_at < restore_at && restore_at < guard_at && guard_at < setup_at,
        "bare setup upgrades to the complete cache lifecycle:\n{text}"
    );
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
    let malformed = velnor_actions_contract::Step {
        name: SETUP_MISE_NAME.to_owned(),
        condition: None,
        kind: velnor_actions_contract::StepKind::Action {
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
