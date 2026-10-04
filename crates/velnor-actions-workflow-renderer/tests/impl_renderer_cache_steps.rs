//! Gate 4 renderer cases: MBX objects, cache actions, lane target dirs.

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_CACHE_MODE_ENV, TOOLS_CACHE_PATH, TOOLS_RESTORE_NAME, TOOLS_SAVE_NAME,
    cache_action_step, mbx_steps_for_driver, target_dir_for_lane, tools_cache_key,
    tools_restore_step, tools_save_step,
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
fn cache_restore_accepts_only_allowed_paths() {
    let uses = format!("actions/cache/restore@{}", sha());
    let key = "velnor-v1-sources-trusted-compat-snapshot".to_owned();
    let paths = vec![
        "$CARGO_HOME/registry".to_owned(),
        "$CARGO_HOME/git".to_owned(),
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
        "$CARGO_HOME/credentials.toml",
        "$RUNNER_TEMP/velnor/target/abc",
        "/abs/path",
        "$HOME/.rustup",
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
            &["$CARGO_HOME/registry".to_owned()]
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

#[test]
fn tools_cache_key_scopes_target_mise_generator_job_and_toolfiles() {
    let key = tools_cache_key("x86_64-unknown-linux-gnu", "2026.9.16", "0.1.0", "plan")
        .expect("tools key");
    assert!(key.starts_with("mise-tools-v1-"), "{key}");
    for part in [
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        "0.1.0",
        "plan",
        "hashFiles('mise.toml','.mise.toml','mise.lock','.mise.lock','.tool-versions')",
    ] {
        assert!(key.contains(part), "key misses {part}: {key}");
    }
    assert!(!key.contains(' ') && !key.contains('\n'), "{key}");
    for bad in [
        ("", "2026.9.16", "0.1.0", "plan"),
        ("x86_64-unknown-linux-gnu", "latest", "0.1.0", "plan"),
        (
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            "0.1.0",
            "velnor plan",
        ),
        (
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            "0.1.0",
            "plan${{x}}",
        ),
        ("wasm32-unknown-unknown", "2026.9.16", "0.1.0", "plan"),
    ] {
        assert!(
            tools_cache_key(bad.0, bad.1, bad.2, bad.3).is_err(),
            "key accepted {bad:?}"
        );
    }
}

#[test]
fn tools_restore_and_save_pin_mise_data_dir_only() {
    let key = tools_cache_key("x86_64-unknown-linux-gnu", "2026.9.16", "0.1.0", "plan")
        .expect("tools key");
    let restore = tools_restore_step(&key).expect("restore");
    assert_eq!(restore.name, TOOLS_RESTORE_NAME);
    let StepKind::Action { uses, with, .. } = &restore.kind else {
        panic!("restore must be an action step");
    };
    assert!(uses.starts_with("actions/cache/restore@"), "{uses}");
    assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
    assert_eq!(with.get("path").map(String::as_str), Some(TOOLS_CACHE_PATH));
    assert!(
        with.get("restore-keys")
            .is_some_and(|keys| keys.starts_with("mise-tools-v1-")),
        "restore carries a prefix key"
    );
    let save = tools_save_step(&key).expect("save");
    assert_eq!(save.name, TOOLS_SAVE_NAME);
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert!(uses.starts_with("actions/cache/save@"), "{uses}");
    assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
    assert_eq!(with.get("path").map(String::as_str), Some(TOOLS_CACHE_PATH));
    assert!(
        !with.contains_key("restore-keys"),
        "save has no restore keys"
    );
    assert!(
        cache_action_step(
            true,
            &format!("actions/cache/restore@{}", sha()),
            "tools",
            &key,
            &[],
            &["$CARGO_HOME/registry".to_owned()]
        )
        .is_err(),
        "tools layer rejects non-mise paths"
    );
}

#[test]
fn strict_restores_builtin_and_saves_on_elected_writer()
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
    let text = strict(&fixture_ir(vec![lint]), &fixture_ctx())?;
    let names = step_names(&text, "actionlint");
    assert!(
        !names.iter().any(|s| s == TOOLS_RESTORE_NAME),
        "P08: restores stay built-in: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|s| *s == TOOLS_SAVE_NAME).count(),
        1,
        "P08: sole owner saves once: {names:?}"
    );
    assert_eq!(
        names.iter().position(|s| s == "Setup Mise"),
        Some(1),
        "setup right after checkout: {names:?}"
    );
    for need in ["cache: \"true\"", "cache_key: mise-v1-"] {
        assert!(text.contains(need), "built-in cache {need}:\n{text}");
    }
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

#[test]
fn lane_target_dirs_stay_isolated() {
    let one = target_dir_for_lane("lane-one");
    let two = target_dir_for_lane("lane-two");
    assert!(one.starts_with("$RUNNER_TEMP/velnor/target/"), "{one}");
    assert_ne!(one, two, "lanes never share a target dir");
}

#[test]
fn mbx_objects_step_gates_save_to_push_via_cache_mode() {
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let [_, direct] = mbx_tool_steps(&uses, "1.19.0", "1.98.1").expect("direct MBX steps");
    let [_, driven] = mbx_steps_for_driver(
        &uses,
        CompileDriver::Mbx,
        "1.19.0",
        "1.98.1",
        mbx_tool_env("1.98.1"),
    )
    .expect("driver MBX")
    .expect("MBX driver emits");
    for step in [&direct, &driven] {
        let StepKind::Action { env, .. } = &step.kind else {
            panic!("mbx must be an action step");
        };
        assert_eq!(
            env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
            Some("read"),
            "the action stays restore-only so its post cannot triple the store"
        );
    }
    assert!(
        mbx_steps_for_driver(
            &uses,
            CompileDriver::Cargo,
            "1.19.0",
            "1.98.1",
            mbx_tool_env("1.98.1"),
        )
        .expect("cargo driver")
        .is_none(),
        "cargo drivers emit no preflight or MBX step to gate"
    );
}
