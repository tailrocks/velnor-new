//! Tofu crate-job obligation tests (T12).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use crate::matrix_step::step_name_for;
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

/// Provider restore present, rust restores absent, for pure-tofu names.
fn assert_provider_restore_only(names: &[&str]) {
    for rust in ["Restore Cargo sources", "Restore MBX objects"] {
        assert!(!names.contains(&rust), "no rust-pinned {rust}: {names:?}");
    }
    assert!(
        names.contains(&"Restore Tofu providers"),
        "pure tofu restores its providers: {names:?}"
    );
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
    assert_eq!(
        obligation_rank(&clippy),
        velnor_actions_rust::task_kind_rank("clippy")
    );
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
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[String::new()],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
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
    assert_provider_restore_only(&names);
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
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![rust, tofu]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
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

/// One fmt/init/validate triple per root, all runnable.
fn tofu_triples(roots: &[&str]) -> Vec<ProposedTask> {
    use velnor_actions_tofu::TofuTaskKind;
    let mut tasks = Vec::new();
    for root in roots {
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            tasks.push(tofu_group(root, kind));
        }
    }
    tasks
}

/// Shell env of one named step.
fn step_env<'a>(
    job: &'a velnor_actions_contract::Job,
    name: &str,
) -> &'a std::collections::BTreeMap<String, String> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .expect("named step");
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("{name} must be a shell step");
    };
    env
}

#[test]
fn tofu_root_jobs_stage_lanes_by_max_parallel() {
    let tasks = tofu_triples(&["stacks/a", "stacks/b", "stacks/c"]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 3, "one job per root");
    let (first, second, third) = (&found.jobs[0], &found.jobs[1], &found.jobs[2]);
    assert_eq!(first.1.needs, vec![PLAN_JOB_ID.to_owned()]);
    assert_eq!(second.1.needs, vec![PLAN_JOB_ID.to_owned()]);
    assert_eq!(
        third.1.needs,
        vec![PLAN_JOB_ID.to_owned(), first.0.clone()],
        "third job waits for the first lane"
    );
}

#[test]
fn wide_cap_stages_nothing() {
    let tasks = tofu_triples(&["stacks/a", "stacks/b"]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 5,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    for (id, job) in &found.jobs {
        assert_eq!(job.needs, vec![PLAN_JOB_ID.to_owned()], "{id} runs free");
    }
}

#[test]
fn rust_jobs_never_stage() {
    let clippy = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let nested = crate_jobs_tests::group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![clippy, nested]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 1,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    for (id, job) in &found.jobs {
        assert_eq!(
            job.needs,
            vec![PLAN_JOB_ID.to_owned()],
            "{id} needs plan only"
        );
        for step in &job.steps {
            if let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind {
                for key in [
                    velnor_actions_workflow_renderer::MATRIX_NEEDS_JOB_ENV,
                    velnor_actions_workflow_renderer::MATRIX_OUTPUT_ENV,
                    velnor_actions_workflow_renderer::MATRIX_MAX_PARALLEL_ENV,
                ] {
                    assert!(!env.contains_key(key), "{id} carries no {key}");
                }
            }
        }
    }
}

#[test]
fn first_tofu_obligation_declares_the_cap() {
    use velnor_actions_workflow_renderer::{
        COVERED_TASKS_OUTPUT, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV,
    };
    let tasks = tofu_triples(&["stacks/a"]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 3,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let job = &found.jobs[0].1;
    let first = step_env(job, "Format");
    assert_eq!(
        first.get(MATRIX_NEEDS_JOB_ENV).map(String::as_str),
        Some("plan")
    );
    assert_eq!(
        first.get(MATRIX_OUTPUT_ENV).map(String::as_str),
        Some(COVERED_TASKS_OUTPUT)
    );
    assert_eq!(
        first.get(MATRIX_MAX_PARALLEL_ENV).map(String::as_str),
        Some("3")
    );
    let later = step_env(job, "Validate");
    for key in [
        MATRIX_NEEDS_JOB_ENV,
        MATRIX_OUTPUT_ENV,
        MATRIX_MAX_PARALLEL_ENV,
    ] {
        assert!(!later.contains_key(key), "later obligations stay quiet");
    }
}

#[test]
fn all_tofu_groups_take_tofu_ids_mixed_keep_rust() {
    use velnor_actions_contract::TOFU_JOB_ID_PREFIX;
    use velnor_actions_tofu::TofuTaskKind;
    let mut rust = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    let tofu = tofu_group("stacks/a", TofuTaskKind::Validate);
    rust.identity.unit_id.clone_from(&tofu.identity.unit_id);
    rust.configuration.clone_from(&tofu.configuration);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &crate_jobs_tests::discovery(vec![
            rust,
            tofu,
            tofu_group("stacks/b", TofuTaskKind::Validate),
        ]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2);
    let mut ids: Vec<&str> = found.jobs.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 2);
    assert!(
        ids[0].starts_with("rust-"),
        "mixed group keeps the rust union: {ids:?}"
    );
    assert_eq!(ids[1], format!("{TOFU_JOB_ID_PREFIX}stacks-b"));
    for (id, job) in &found.jobs {
        assert!(
            velnor_actions_contract::is_crate_job_id(id),
            "{id} stays a crate job"
        );
        assert_eq!(job.needs, vec![PLAN_JOB_ID.to_owned()], "{id} needs plan");
    }
}
