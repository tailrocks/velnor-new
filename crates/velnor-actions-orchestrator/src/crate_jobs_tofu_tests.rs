//! Tofu crate-job obligation tests (T12).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use velnor_actions_rust::TaskKind;

/// Tofu proposal via the T12 adapter constructor.
fn tofu_group(root: &str, kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: root.to_owned(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn tofu_obligations_order_fmt_init_validate() {
    use velnor_actions_tofu::TofuTaskKind;
    let fmt = tofu_group("", TofuTaskKind::Fmt);
    let init = tofu_group("", TofuTaskKind::InitForValidate);
    let validate = tofu_group("", TofuTaskKind::Validate);
    assert!(obligation_rank(&fmt) < obligation_rank(&init));
    assert!(obligation_rank(&init) < obligation_rank(&validate));
    let clippy = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    assert_eq!(obligation_rank(&clippy), task_kind_rank("clippy"));
    let tasks = vec![&validate, &fmt, &init];
    let obligations = obligations_for(&tasks, &ToolCatalog::pinned()).expect("obligations build");
    let kinds: Vec<&str> = obligations
        .iter()
        .map(|obligation| obligation.kind.as_str())
        .collect();
    assert_eq!(kinds, vec!["fmt", "init", "validate"]);
    assert_eq!(obligations[0].step_name, step_name_for("fmt", &fmt.task_id));
    assert_eq!(obligations[2].gated_by, vec![init.task_id.clone()]);
}

#[test]
fn tofu_tasks_bind_no_rust_tools_but_select_opentofu() {
    use velnor_actions_tofu::TofuTaskKind;
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    let needs = needs(&tofu);
    assert!(!needs.mbx && !needs.nextest);
    assert!(!is_mbx(&tofu) && !is_nextest(&tofu));
    assert!(is_opentofu(&tofu));
    assert!(!is_rust(&tofu), "tofu tasks need no Rust toolchain");
    let rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    assert!(!is_opentofu(&rust));
    assert!(is_rust(&rust), "rust tasks keep the toolchain");
}

#[test]
fn pure_tofu_group_renders_without_rust_setup() {
    use velnor_actions_contract::StepKind;
    use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP};
    use velnor_actions_tofu::TofuTaskKind;

    use crate::source_prep::FETCH_SOURCES_STEP;
    let tasks = vec![
        tofu_group("stacks/a", TofuTaskKind::Fmt),
        tofu_group("stacks/a", TofuTaskKind::InitForValidate),
        tofu_group("stacks/a", TofuTaskKind::Validate),
    ];
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(tasks),
        &ToolCatalog::pinned(),
        &[String::new()],
        &[],
        None,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "one job per tofu root");
    let job = &found.jobs[0].1;
    let names = crate_jobs_tests::names(job);
    assert!(
        !names.contains(&PREPARE_RUST_COMPONENTS_STEP),
        "no components step: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with(FETCH_SOURCES_STEP)),
        "no {FETCH_SOURCES_STEP} despite lockful roots: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("Restore")),
        "no rust-pinned restore: {names:?}"
    );
    let catalog = ToolCatalog::pinned();
    let prepare = job
        .steps
        .iter()
        .find(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let StepKind::Shell { run, env } = &prepare.kind else {
        panic!("prepare must be a shell step");
    };
    assert!(
        !run.contains(&catalog.tool_spec(PinnedTool::Rust)),
        "prepare installs no Rust: {run:?}"
    );
    assert!(
        run.contains(&catalog.tool_spec(PinnedTool::Opentofu)),
        "prepare installs the opentofu driver: {run:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(!env.contains_key(key), "prepare carries no {key}: {env:?}");
    }
    let validate = job
        .steps
        .iter()
        .find(|step| step.name == "Validate")
        .expect("validate obligation");
    let StepKind::Shell { env, .. } = &validate.kind else {
        panic!("obligation must be a shell step");
    };
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(
            !env.contains_key(key),
            "tofu obligation carries no {key}: {env:?}"
        );
    }
    assert!(
        env.get(velnor_actions_tofu::TF_DATA_DIR_ENV)
            .is_some_and(|dir| dir.contains("tofu-data")),
        "tofu obligation keeps its isolated data dir: {env:?}"
    );
}

#[test]
fn mixed_group_keeps_the_rust_union() {
    use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP};
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &crate_jobs_tests::discovery(vec![rust, tofu]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1, "shared group renders once");
    let job = &found.jobs[0].1;
    let names = crate_jobs_tests::names(job);
    assert!(
        names.contains(&PREPARE_RUST_COMPONENTS_STEP),
        "mixed groups keep components: {names:?}"
    );
    let catalog = ToolCatalog::pinned();
    let prepare = job
        .steps
        .iter()
        .find(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let velnor_actions_contract::StepKind::Shell { run, env } = &prepare.kind else {
        panic!("prepare must be a shell step");
    };
    assert!(
        run.contains(&catalog.tool_spec(PinnedTool::Rust))
            && run.contains(&catalog.tool_spec(PinnedTool::Opentofu)),
        "mixed groups install the union: {run:?}"
    );
    assert!(
        env.contains_key("RUSTUP_TOOLCHAIN"),
        "mixed prepare keeps the triple: {env:?}"
    );
}
