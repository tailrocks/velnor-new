//! Gate 4 renderer cases: MBX objects, cache actions, lane target dirs.

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::steps::{
    cache_action_step, mbx_objects_step, target_dir_for_lane,
};

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
fn mbx_objects_step_pins_action_and_mode() {
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let step = mbx_objects_step(&uses, false).expect("mbx");
    assert_eq!(kind_name(&step), "action");
    match &step.kind {
        StepKind::Action { uses: got, with } => {
            assert!(got.starts_with("jdx/mr-boxington-action@"), "{got}");
            assert_eq!(with.get("mode").map(String::as_str), Some("objects"));
        }
        _ => panic!("mbx must be an action step"),
    }
    assert!(
        mbx_objects_step(&uses, true).is_err(),
        "cargo profiles never emit MBX"
    );
    let other = format!("actions/cache/restore@{}", sha());
    assert!(
        mbx_objects_step(&other, false).is_err(),
        "wrong action rejected"
    );
    assert!(
        mbx_objects_step("jdx/mr-boxington-action@main", false).is_err(),
        "unpinned rejected"
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
fn lane_target_dirs_stay_isolated() {
    let one = target_dir_for_lane("lane-one");
    let two = target_dir_for_lane("lane-two");
    assert!(one.starts_with("$RUNNER_TEMP/velnor/target/"), "{one}");
    assert_ne!(one, two, "lanes never share a target dir");
}
