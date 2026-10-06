//! Gate 4 renderer cases: cache actions, lane target dirs, and V2 tools.

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::steps::{
    TOOLS_CACHE_PATHS, TOOLS_RESTORE_NAME, TOOLS_SAVE_NAME, cache_action_step,
};

use super::impl_renderer_fixtures::*;

/// Pinned refs used across cache-step cases.
fn sha() -> &'static str {
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}

/// Step kinds stay closed: action, shell, internal — no parallel syntax.
fn kind_name(step: &Step) -> &'static str {
    match &step.kind {
        StepKind::Action { .. } => "action",
        StepKind::Shell { .. } => "shell",
        StepKind::Internal { .. } => "internal",
    }
}

#[test]
fn step_kinds_have_no_parallel_syntax() {
    let uses = format!("actions/checkout@{}", sha());
    let step = velnor_actions_workflow_renderer::steps::checkout_step(&uses).expect("checkout");
    assert_eq!(kind_name(&step), "action");
    assert_eq!(
        kind_name(&velnor_actions_workflow_renderer::steps::plan_step()),
        "internal"
    );
}

#[test]
fn cache_restore_accepts_only_owned_paths() {
    let uses = format!("actions/cache/restore@{}", sha());
    let key = "velnor-v1-sources-trusted-compat-snapshot".to_owned();
    let home = "${{ runner.temp }}/velnor/cargo";
    let paths = vec![
        format!("{home}/registry/index"),
        format!("{home}/registry/cache"),
        format!("{home}/git/db"),
    ];
    let step = cache_action_step(
        true,
        &uses,
        "sources",
        &key,
        &["velnor-v1-".to_owned()],
        &paths,
    )
    .expect("restore");
    match &step.kind {
        StepKind::Action { with, .. } => {
            assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
            assert_eq!(
                with.get("restore-keys").map(String::as_str),
                Some("velnor-v1-"),
                "restore-keys pass through verbatim; the renderer never computes identity"
            );
        }
        _ => panic!("restore must be an action step"),
    }
    for bad in [
        "$CARGO_HOME/registry",
        "$CARGO_HOME/git",
        "$RUNNER_TEMP/velnor/target/abc",
        "/abs/path",
        "${{ runner.temp }}/velnor/cargo/credentials.toml",
        "${{ runner.temp }}/velnor/cargo/.crates.toml",
        "${{ runner.temp }}/velnor/cargo/bin",
        "~/.rustup",
    ] {
        assert!(
            cache_action_step(true, &uses, "sources", &key, &[], &[bad.to_owned()]).is_err(),
            "{bad}"
        );
    }
    assert!(
        cache_action_step(true, &uses, "mbx", &key, &[], &paths).is_err(),
        "mbx needs objects"
    );
    let save = format!("actions/cache/save@{}", sha());
    assert!(
        cache_action_step(true, &save, "sources", &key, &[], &paths).is_err(),
        "wrong op"
    );
}

#[test]
fn tools_layer_accepts_only_its_exact_v2_payload_paths() {
    let uses = format!("actions/cache/restore@{}", sha());
    let key = "mise-tools-v2-${{steps.v2.outputs.identity}}";
    let paths = TOOLS_CACHE_PATHS.map(str::to_owned);
    let restore = cache_action_step(true, &uses, "tools", key, &[], &paths).expect("restore");
    let StepKind::Action { with, .. } = restore.kind else {
        panic!("tools restore must be an action step");
    };
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(paths.join("\n").as_str())
    );
    for bad in [
        "${{ runner.temp }}/velnor/cargo/registry",
        "$CARGO_HOME/registry",
        "${{ runner.temp }}/velnor/target",
    ] {
        assert!(
            cache_action_step(true, &uses, "tools", key, &[], &[bad.to_owned()]).is_err(),
            "tools layer accepted {bad}"
        );
    }
}

#[test]
fn cache_save_writes_task_artifacts_only() {
    let uses = format!("actions/cache/save@{}", sha());
    let key = "velnor-v1-task-trusted-compat-snapshot".to_owned();
    let task_dir = "$MISE_TASK_CACHE_DIR/task-artifacts/v2".to_owned();
    let step = cache_action_step(false, &uses, "task", &key, &[], &[task_dir]).expect("save");
    match &step.kind {
        StepKind::Action { with, .. } => {
            assert!(
                !with.contains_key("restore-keys"),
                "save has no restore keys"
            );
        }
        _ => panic!("save must be an action step"),
    }
    assert!(
        cache_action_step(
            false,
            &uses,
            "task",
            &key,
            &[],
            &["${{ runner.temp }}/velnor/cargo/registry/cache".to_owned()]
        )
        .is_err()
    );
    assert!(
        cache_action_step(
            false,
            &uses,
            "task",
            "",
            &[],
            &["$MISE_TASK_CACHE_DIR/task-artifacts/v2".to_owned()]
        )
        .is_err()
    );
}

fn assert_save_gate_and_seed(
    rendered: &velnor_actions_workflow_renderer::render::RenderedWorkflow,
    text: &str,
) -> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::RenderError;
    let save_at = text
        .find("- name: Save Mise tools")
        .ok_or_else(|| RenderError::InvalidWorkflow("save step".to_owned()))?;
    let save_step = text[save_at..]
        .split("      - name:")
        .next()
        .ok_or_else(|| RenderError::InvalidWorkflow("save boundary".to_owned()))?;
    for gate in [
        "github.event_name == 'push'",
        "github.ref_protected == true",
        "steps.v2.outputs.enabled == 'true'",
    ] {
        if !save_step.contains(gate) {
            return Err(RenderError::InvalidWorkflow(format!(
                "save requires protected-push gate {gate}:\n{save_step}",
            )));
        }
    }
    let seed = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/velnor-tool-seed/action.yml")
        .ok_or_else(|| RenderError::InvalidWorkflow("renderer emits seed action".to_owned()))?;
    if !seed.bytes.contains("velnor-host-seed-v1") {
        return Err(RenderError::InvalidWorkflow(format!(
            "seed provenance missing:\n{}",
            seed.bytes
        )));
    }
    if !text.contains("${{steps.v2.outputs.identity}}") {
        return Err(RenderError::InvalidWorkflow(
            "v2 identity output missing".to_owned(),
        ));
    }
    Ok(())
}

#[test]
fn strict_wires_runtime_qualified_tools_cache_before_setup_and_saves_once()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::checkout_step;
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
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &fixture_ir(vec![lint]),
        velnor_actions_contract::WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )?;
    let text = &rendered.yaml;
    let names = step_names(text, "actionlint");
    assert_eq!(names.iter().filter(|s| *s == TOOLS_RESTORE_NAME).count(), 1);
    assert_eq!(names.iter().filter(|s| *s == TOOLS_SAVE_NAME).count(), 1);
    assert_eq!(
        names.iter().position(|s| s == "Setup Mise"),
        Some(3),
        "checkout, combined runtime identity and seed, restore, then setup: {names:?}"
    );
    assert_eq!(
        names.iter().position(|s| s == TOOLS_RESTORE_NAME),
        Some(2),
        "archive restore follows seed admission: {names:?}"
    );
    assert!(!text.contains("name: Restore Velnor tool seed"), "{text}");
    for need in [
        "name: V2 identity",
        "id: v2",
        "outputs.enabled == 'true'",
        "mise-tools-v2-",
        "cache: \"false\"",
        "cache_save: \"false\"",
        "path:",
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo/.crates.toml",
        "${{ runner.temp }}/velnor/cargo/.crates2.json",
        "${{ runner.temp }}/velnor/cargo/bin",
    ] {
        assert!(
            text.contains(need),
            "V2 tools cache missing {need}:\n{text}"
        );
    }
    assert!(
        !text.contains("cache_key: mise-v1-"),
        "legacy Mise cache key:\n{text}"
    );
    let setup = text
        .split("      - name: Setup Mise")
        .nth(1)
        .unwrap_or_default();
    let setup = setup.split("      - name:").next().unwrap_or_default();
    assert!(
        !setup.contains("cache_key:"),
        "Mise does not own an archive key"
    );
    assert_save_gate_and_seed(&rendered, text)?;
    Ok(())
}

#[test]
fn seed_and_tools_prelude_require_the_configured_unconditional_checkout()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::checkout_step;
    let shell_checkout = scrubbed_shell_step("Checkout", vec!["true".to_owned()])?;
    let run = || {
        let job = job(
            "actionlint",
            "Actionlint",
            Vec::new(),
            vec![
                shell_checkout.clone(),
                scrubbed_shell_step(
                    "Run actionlint",
                    mise_argv("actionlint@1.7.12", "actionlint", &[]),
                )?,
            ],
        );
        strict(&fixture_ir(vec![job]), &fixture_ctx())
    };
    let text = run()?;
    assert!(!text.contains("name: V2 identity"), "{text}");
    assert!(!text.contains(TOOLS_RESTORE_NAME), "{text}");
    assert!(!text.contains("Restore Velnor tool seed"), "{text}");

    let mut renamed = checkout_step(&checkout_pin())?;
    renamed.name = "Source checkout".to_owned();
    let real = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            renamed,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &[]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![real]), &fixture_ctx())?;
    assert!(
        text.contains("name: V2 identity"),
        "renamed typed checkout: {text}"
    );
    assert!(
        text.contains("uses: ./.github/actions/velnor-tools-prelude-u26"),
        "renamed typed checkout: {text}"
    );

    let mut conditional = checkout_step(&checkout_pin())?;
    conditional.condition = Some("false".to_owned());
    let conditional = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            conditional,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &[]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![conditional]), &fixture_ctx())?;
    assert!(!text.contains("name: V2 identity"), "{text}");
    assert!(!text.contains("Restore Velnor tool seed"), "{text}");
    Ok(())
}

#[test]
fn strict_leaves_setup_less_jobs_without_tools_cache()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::{checkout_step, merge_step, plan_step};
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
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    );
    final_job.1.condition = Some("always()".to_owned());
    let text = strict(&fixture_ir(vec![plan, final_job]), &fixture_ctx())?;
    let names = step_names(&text, "required");
    assert!(
        !names
            .iter()
            .any(|s| s == TOOLS_RESTORE_NAME || s == TOOLS_SAVE_NAME),
        "setup-less job must not cache: {names:?}"
    );
    Ok(())
}
