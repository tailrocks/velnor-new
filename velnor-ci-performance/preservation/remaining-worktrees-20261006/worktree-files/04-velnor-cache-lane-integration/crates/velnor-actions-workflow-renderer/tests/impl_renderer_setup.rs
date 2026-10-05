//! Pinned Mise setup emission: template shape plus strict insertion.
use std::collections::BTreeMap;
use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::{
    MiseSetup, RenderError, SETUP_MISE_NAME, checkout_step, mise_setup_step, plan_step,
    steps::{WORKSPACE_CACHE_GUARD_NAME, workspace_cache_guard_step},
};

use super::impl_renderer_fixtures::*;

#[path = "impl_renderer_setup_cache_guard.rs"]
mod cache_guard_tests;

#[test]
fn setup_step_shape_exact() -> Result<(), RenderError> {
    let step = mise_setup_step(&mise())?;
    assert_eq!(step.name, SETUP_MISE_NAME);
    let velnor_actions_contract::StepKind::Action { uses, with, env } = &step.kind else {
        panic!("setup must be an action step");
    };
    assert_eq!(uses, MISE_USES);
    assert_eq!(
        env,
        &BTreeMap::from([(
            "MISE_DATA_DIR".to_owned(),
            "${{ github.workspace }}/.velnor/cache/mise".to_owned(),
        )])
    );
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
    let names = step_names(&text, "actionlint");
    assert_eq!(
        &names[..4],
        [
            "Checkout",
            WORKSPACE_CACHE_GUARD_NAME,
            "Restore Mise tools",
            "Setup Mise",
        ]
    );
    let setup_at = text.find(SETUP_MISE_NAME).expect("setup inserted");
    let exclusion_at = text
        .find(&format!("- name: {WORKSPACE_CACHE_GUARD_NAME}"))
        .expect("workspace cache exclusion");
    let restore_at = text
        .find("- name: Restore Mise tools")
        .expect("tools restore");
    let mise_at = text.find("mise --no-config").expect("mise step kept");
    assert!(setup_at < mise_at, "setup must precede mise:\n{text}");
    assert_eq!(
        text.matches(&format!("- name: {WORKSPACE_CACHE_GUARD_NAME}"))
            .count(),
        1
    );
    assert!(
        exclusion_at < restore_at,
        "exclusion before restore:\n{text}"
    );
    assert!(restore_at < setup_at, "restore precedes setup:\n{text}");
    assert_eq!(WORKSPACE_CACHE_GUARD_NAME, "Guard Velnor cache paths");
    let exclusion = workspace_cache_guard_step(true)?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &exclusion.kind else {
        panic!("workspace cache exclusion must be a shell step");
    };
    let script = run.get(2).expect("shell command carries its script");
    for marker in [
        "rule='/.velnor/cache/'",
        "if [ -L \"$workspace/.git\" ] || [ ! -d \"$workspace/.git\" ];",
        "if [ -L \"$workspace/.git/info\" ] || [ ! -d \"$workspace/.git/info\" ];",
        "if [ -L \"$exclude\" ] || { [ -e \"$exclude\" ] && [ ! -f \"$exclude\" ]; };",
        ".velnor/cache/cargo/registry/index",
        ".velnor/cache/cargo/registry/cache",
        ".velnor/cache/cargo/registry/src",
        ".velnor/cache/cargo/git",
        ".velnor/cache/cargo/git/db",
        ".velnor/cache/cargo/git/checkouts",
        "grep -Fqx \"$rule\" \"$exclude\"",
        "printf '\\n%s\\n' \"$rule\" >> \"$exclude\"",
        "git -C \"$workspace\" check-ignore -q .velnor/cache/seed",
    ] {
        assert!(
            script.contains(marker),
            "cache exclusion script misses {marker}:\n{script}"
        );
    }
    for line in [
        format!("uses: {MISE_USES}"),
        format!("version: {MISE_VERSION}"),
        format!("sha256: {MISE_SHA256}"),
        "install: \"false\"".to_owned(),
        "env: \"false\"".to_owned(),
        "cache: \"false\"".to_owned(),
        "cache_save: \"false\"".to_owned(),
        "cache_key: mise-v1-".to_owned(),
        "MISE_DATA_DIR: ${{ github.workspace }}/.velnor/cache/mise".to_owned(),
    ] {
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(
        !text.contains("cache: \"true\""),
        "Mise cache disabled:\n{text}"
    );
    assert!(
        !text.contains("~/.local/share/mise"),
        "no HOME-derived Mise path:\n{text}"
    );
    for line in [
        "- name: Save Mise tools".to_owned(),
        "key: mise-v1-".to_owned(),
        "if: success() && github.event_name == 'push'".to_owned(),
    ] {
        assert!(text.contains(&line), "sole owner saves {line}:\n{text}");
    }
    Ok(())
}

#[test]
fn strict_required_without_checkout_keeps_no_checkout() -> Result<(), RenderError> {
    let required = job(
        "required",
        "Required",
        Vec::new(),
        vec![scrubbed_shell_step(
            "Run required checks",
            mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
        )?],
    );
    let text = strict(&fixture_ir(vec![required]), &fixture_ctx())?;
    let names = step_names(&text, "required");
    assert_eq!(
        &names[..3],
        [
            WORKSPACE_CACHE_GUARD_NAME,
            "Restore Mise tools",
            "Setup Mise"
        ]
    );
    assert!(
        !names.iter().any(|name| name == "Checkout"),
        "Required does not gain a checkout: {names:?}"
    );
    Ok(())
}

#[test]
fn strict_rejects_cache_restore_before_checkout() -> Result<(), RenderError> {
    let restore = velnor_actions_workflow_renderer::steps::tools_restore_step(
        "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa",
    )?;
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            restore,
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    assert!(
        strict(&fixture_ir(vec![lint]), &fixture_ctx())
            .is_err_and(|err| { format!("{err:?}").contains("cache_restore_before_checkout") })
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
