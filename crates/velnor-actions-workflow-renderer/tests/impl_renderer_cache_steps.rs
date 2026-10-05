//! Gate 4 renderer cases: MBX objects, cache actions, lane target dirs.

use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_CACHE_MODE_ENV, TOOLS_RESTORE_NAME, TOOLS_SAVE_NAME, action_step,
    action_step_with_env, cache_action_step, mbx_objects_step, mbx_step_for_driver,
    target_dir_for_lane,
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
fn mbx_domain_expression_allowlist_is_exact() {
    let trust = "github.event_name == 'push' && github.ref_protected && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && 'trusted' || 'pr'";
    let mode = "github.event_name == 'push' && github.ref_protected && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && 'write' || 'read'";
    let key = format!("domain-${{{{ {trust} }}}}-${{{{ github.run_id }}}}");
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let allowed = action_step(
        "MBX",
        &uses,
        std::collections::BTreeMap::from([("cache-key".to_owned(), key.clone())]),
    );
    assert!(allowed.is_ok(), "exact domain expression allowed: {key}");
    assert!(
        action_step_with_env(
            "MBX",
            &uses,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::from([(
                MBX_CACHE_MODE_ENV.to_owned(),
                format!("${{{{ {mode} }}}}")
            )]),
        )
        .is_ok(),
        "exact protected-default writer mode is allowlisted"
    );

    for bad in [
        key.replace("github.ref_protected", "env.CI"),
        key.replace("github.event.repository.default_branch", "github.ref_name"),
        key.replace("'trusted' || 'pr'", "'write' || 'read'"),
    ] {
        assert!(
            action_step(
                "MBX",
                &uses,
                std::collections::BTreeMap::from([("cache-key".to_owned(), bad.clone())]),
            )
            .is_err(),
            "unowned cache-key expression rejected: {bad}"
        );
    }
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
            assert_eq!(
                with.get("cache-generation").map(String::as_str),
                Some(mbx_cache_generation("1.19.0").as_str()),
                "a new MBX release starts an isolated cache namespace"
            );
            for input in [
                "save-on-pull-request",
                "save-on-workflow-dispatch",
                "save-on-protected-branch",
            ] {
                assert!(
                    !with.contains_key(input),
                    "consumer cache writes stay push-only: {input}"
                );
            }
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
        "${{ runner.temp }}/velnor/cargo/registry/cache".to_owned(),
        "${{ runner.temp }}/velnor/cargo/registry/index".to_owned(),
        "${{ runner.temp }}/velnor/cargo/git/db".to_owned(),
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
        "$CARGO_HOME/registry",
        "$CARGO_HOME/git",
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
    assert_eq!(step.condition.as_deref(), Some(CACHE_SAVE_CONDITION));
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
    let driven = mbx_step_for_driver(&uses, CompileDriver::Mbx, "1.19.0")
        .expect("driver mbx")
        .expect("mbx driver emits");
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
        mbx_step_for_driver(&uses, CompileDriver::Cargo, "1.19.0")
            .expect("cargo driver")
            .is_none(),
        "cargo drivers emit no MBX step to gate"
    );
}
