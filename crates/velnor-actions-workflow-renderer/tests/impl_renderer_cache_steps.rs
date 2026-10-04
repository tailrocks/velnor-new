//! Gate 4 renderer cases: MBX objects, cache actions, lane target dirs.

use velnor_actions_contract::workflow::ir::CACHE_MODE_PUSH_WRITE_EXPR;
use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    CompilerDriver, MBX_CACHE_MODE_ENV, TOOLS_RESTORE_NAME, TOOLS_SAVE_NAME, cache_action_step,
    checkout_step, mbx_objects_step, mbx_step_for_driver, target_dir_for_lane, tool_payload_paths,
    tools_restore_step, tools_save_step,
};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use velnor_actions_workflow_renderer::cache_p08::mise_cache_key_for_tools;

use super::impl_renderer_fixtures::*;

/// Pinned refs used across cache-step cases.
fn sha() -> &'static str {
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}

/// Step kinds stay closed; source helpers carry their own authority.
fn kind_name(step: &Step) -> &'static str {
    match &step.kind {
        StepKind::Action { .. } => "action",
        StepKind::Shell { .. } => "shell",
        StepKind::Internal { .. } => "internal",
        StepKind::SourceBoundHelper { .. } => "source-bound-helper",
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
fn mbx_objects_step_pins_action_and_mode() {
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let step = mbx_objects_step(&uses, false, "1.19.0").expect("mbx");
    assert_eq!(kind_name(&step), "action");
    match &step.kind {
        StepKind::Action {
            uses: got, with, ..
        } => {
            assert!(got.starts_with("jdx/mr-boxington-action@"), "{got}");
            assert_eq!(
                with.get("github-cache-mode").map(String::as_str),
                Some("objects")
            );
            assert_eq!(
                with.get("version").map(String::as_str),
                Some("1.19.0"),
                "action installs the exact catalog pin, never latest"
            );
            assert!(!with.contains_key("mode"), "no such action input");
        }
        _ => panic!("mbx must be an action step"),
    }
    assert!(
        mbx_objects_step(&uses, true, "1.19.0").is_err(),
        "cargo profiles never emit MBX"
    );
    let other = format!("actions/cache/restore@{}", sha());
    assert!(
        mbx_objects_step(&other, false, "1.19.0").is_err(),
        "wrong action rejected"
    );
    assert!(
        mbx_objects_step("jdx/mr-boxington-action@main", false, "1.19.0").is_err(),
        "unpinned rejected"
    );
    for bad in ["latest", "v1.19.0", "1.19", "1.19.0.1", "1.19.x", ""] {
        assert!(
            mbx_objects_step(&uses, false, bad).is_err(),
            "loose mbx version {bad:?} must fail"
        );
    }
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
fn tools_cache_key_uses_only_explicit_catalog_and_platform_identity() {
    let specs = ["rust@1.98.1".to_owned()];
    let key = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &specs)
        .expect("tools key");
    assert!(key.starts_with("mise-v3-x86_64-unknown-linux-gnu-2026.9.16-"));
    assert!(
        !key.contains("hashFiles"),
        "repository toolfiles never affect isolated catalog"
    );
    for (target, mise) in [
        ("", "2026.9.16"),
        ("x86_64-unknown-linux-gnu", "latest"),
        ("wasm32-unknown-unknown", "2026.9.16"),
    ] {
        assert!(mise_cache_key_for_tools(target, mise, &specs).is_err());
    }
}

#[test]
fn tools_restore_and_save_share_complete_ordered_payload_and_implementation() {
    let key = "mise-v3-x86_64-unknown-linux-gnu-2026.9.16-aaaaaaaaaaaaaaaa";
    let restore = tools_restore_step(key).expect("restore");
    let save = tools_save_step(key).expect("save");
    assert_eq!(restore.name, TOOLS_RESTORE_NAME);
    assert_eq!(save.name, TOOLS_SAVE_NAME);
    let StepKind::Action {
        uses: restore_uses,
        with: restore_with,
        ..
    } = &restore.kind
    else {
        panic!("restore action");
    };
    let StepKind::Action {
        uses: save_uses,
        with: save_with,
        ..
    } = &save.kind
    else {
        panic!("save action");
    };
    assert_eq!(
        restore_uses.split_once('@').expect("pin").1,
        save_uses.split_once('@').expect("pin").1
    );
    let expected = [
        "${{ runner.temp }}/velnor/mise",
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo/bin",
        "${{ runner.temp }}/velnor/cargo/.crates.toml",
        "${{ runner.temp }}/velnor/cargo/.crates2.json",
    ]
    .join("\n");
    assert_eq!(tool_payload_paths().join("\n"), expected);
    assert_eq!(restore_with.get("path"), save_with.get("path"));
    assert_eq!(restore_with.get("path"), Some(&expected));
    assert_eq!(restore_with.get("key").map(String::as_str), Some(key));
    assert_eq!(restore_with.get("key"), save_with.get("key"));
    assert!(
        !restore_with.contains_key("restore-keys"),
        "no unsafe broad tool fallback"
    );
    assert!(!save_with.contains_key("restore-keys"));
    for bad in [
        "$CARGO_HOME/registry",
        "${{ runner.temp }}/velnor/cargo",
        "~/.local/share/mise",
    ] {
        assert!(
            cache_action_step(true, restore_uses, "tools", key, &[], &[bad.to_owned()]).is_err(),
            "tool archive must reject unowned {bad}"
        );
    }
}

#[test]
fn strict_restores_explicit_payload_without_consumer_export()
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
        names.iter().any(|s| s == TOOLS_RESTORE_NAME),
        "explicit restore present: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|s| *s == TOOLS_SAVE_NAME).count(),
        0,
        "consumer never saves executable payload: {names:?}"
    );
    let restore = names
        .iter()
        .position(|s| s == TOOLS_RESTORE_NAME)
        .expect("restore");
    let setup = names
        .iter()
        .position(|s| s == velnor_actions_workflow_renderer::SETUP_MISE_NAME)
        .expect("bootstrap");
    assert!(
        restore < setup,
        "restored executable verified by bootstrap: {names:?}"
    );
    for need in [
        "VELNOR_MISE_VERSION:",
        "VELNOR_MISE_SHA256:",
        "key: mise-v3-",
    ] {
        assert!(text.contains(need), "explicit authority {need}:\n{text}");
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
    let direct = mbx_objects_step(&uses, false, "1.19.0").expect("mbx");
    let driven = mbx_step_for_driver(&uses, CompilerDriver::Mbx, "1.19.0")
        .expect("driver mbx")
        .expect("mbx driver emits");
    for step in [&direct, &driven] {
        let StepKind::Action { env, .. } = &step.kind else {
            panic!("mbx must be an action step");
        };
        assert_eq!(
            env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
            Some(CACHE_MODE_PUSH_WRITE_EXPR),
            "every MBX step pins the push-only cache mode"
        );
    }
    assert!(
        mbx_step_for_driver(&uses, CompilerDriver::Cargo, "1.19.0")
            .expect("cargo driver")
            .is_none(),
        "cargo drivers emit no MBX step to gate"
    );
    // The mode expression must branch on the event: a constant `write`
    // would reopen PR saves, a constant `read` would break push saves.
    assert!(CACHE_MODE_PUSH_WRITE_EXPR.contains("github.event_name == 'push'"));
    assert!(CACHE_MODE_PUSH_WRITE_EXPR.contains("'write'"));
    assert!(CACHE_MODE_PUSH_WRITE_EXPR.contains("'read'"));
}

#[test]
fn action_step_env_renders_only_when_present() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let mbx = mbx_objects_step(&uses, false, "1.19.0")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job("demo", "Demo", Vec::new(), vec![plain, mbx])]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains(&format!(
            "{MBX_CACHE_MODE_ENV}: {CACHE_MODE_PUSH_WRITE_EXPR}"
        )),
        "mbx mode renders on the step:\n{text}"
    );
    assert_eq!(
        text.matches("env:").count(),
        1,
        "env-less action steps render no env map:\n{text}"
    );
    Ok(())
}
