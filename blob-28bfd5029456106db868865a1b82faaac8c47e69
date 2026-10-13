//! Crate-job construction tests.
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;
use crate::crate_job_ids::job_id_for_member;
use crate::matrix_step::{shard_suffix, step_name_for};
use std::collections::BTreeSet;
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

/// Runnable fixture proposal for one package/kind pair.
pub(super) fn group(package: &str, kind: TaskKind, gated_by: &[&str]) -> ProposedTask {
    let key = if package == "demo" { "root" } else { package };
    let group = TaskGroup {
        task_id: format!("stack/rust/{key}/{}/default", kind.as_str()),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: key.to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: gated_by.iter().map(ToString::to_string).collect(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Discovery shell carrying only task proposals.
pub(super) fn discovery(groups: Vec<ProposedTask>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: groups,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// Step names of one built job.
pub(super) fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

#[test]
fn groups_obligations_into_one_ordered_job_per_crate() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let doctest = group("demo", TaskKind::Doctest, &[clippy_id.as_str()]);
    let doc = group("demo", TaskKind::Doc, &[clippy_id.as_str()]);
    let other = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![test, doc, doctest, clippy, other]),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 2, "one job per crate");
    let ids: Vec<&str> = found.jobs.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["rust-demo", "rust-nested"]);
    for id in &ids {
        assert!(!id.contains("velnor"), "unbranded {id}");
    }
    let demo = &found.jobs[0].1;
    assert_eq!(demo.display_name, "Rust / demo");
    assert_eq!(demo.needs, vec![PLAN_JOB_ID.to_owned()]);
    let steps = names(demo);
    let at = |name: &str| steps.iter().position(|seen| *seen == name);
    let (
        Some(checkout),
        Some(prepare),
        Some(components),
        Some(lint),
        Some(run),
        Some(doctests),
        Some(docs),
    ) = (
        at("Checkout"),
        at("Prepare pinned tools"),
        at("Prepare Rust components"),
        at("Clippy"),
        at("Unit and integration tests"),
        at("Doctests"),
        at("Documentation"),
    )
    else {
        panic!("crate steps out of shape: {steps:?}");
    };
    assert!(checkout < prepare && prepare < components, "{steps:?}");
    assert!(
        components < lint && lint < run && run < doctests && doctests < docs,
        "{steps:?}"
    );
}

#[test]
fn skips_testless_and_workspace_groups() {
    let mut testless = group("demo", TaskKind::Doctest, &[]);
    testless.no_targets = true;
    let mut workspace_fmt = group("demo", TaskKind::Fmt, &[]);
    workspace_fmt.identity.unit_id.clear();
    workspace_fmt.display_name.clear();
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![testless, workspace_fmt, clippy]),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let steps = names(&found.jobs[0].1);
    assert!(steps.contains(&"Clippy"), "{steps:?}");
    assert!(!steps.contains(&"Doctests"), "{steps:?}");
    assert!(!steps.contains(&"Format"), "{steps:?}");
}

#[test]
fn member_binding_agrees_with_built_jobs() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let test = group("demo", TaskKind::Test, &[]);
    let mut workspace_fmt = group("demo", TaskKind::Fmt, &[]);
    workspace_fmt.identity.unit_id.clear();
    workspace_fmt.display_name.clear();
    let groups = vec![clippy, test, workspace_fmt];
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(groups.clone()),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let built = found.jobs[0].0.as_str();
    assert_eq!(
        job_id_for_member(&groups, &groups[0]).as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(&groups, &groups[1]).as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(&groups, &groups[2]).as_deref(),
        Some(PLAN_JOB_ID)
    );
    let outsider = group("other", TaskKind::Clippy, &[]);
    assert_eq!(job_id_for_member(&groups, &outsider), None);
}

#[test]
fn gates_keep_same_crate_edges_only() {
    let clippy_id = "stack/rust/root/clippy/default".to_owned();
    let mut doc = group("demo", TaskKind::Doc, &[]);
    doc.gated_by = vec![
        clippy_id.clone(),
        "stack/rust/foreign/clippy/default".to_owned(),
    ];
    doc.depends_on = vec![clippy_id.clone()];
    let executed: BTreeSet<&str> = [clippy_id.as_str(), doc.task_id.as_str()]
        .into_iter()
        .collect();
    assert_eq!(gates_for(&doc, &executed), [clippy_id]);
}

#[test]
fn shards_name_their_index() {
    assert_eq!(shard_suffix("stack/rust/root/nextest/default"), None);
    assert_eq!(
        shard_suffix("stack/rust/root/nextest/default/shard-2-of-4"),
        Some((2, 4))
    );
    assert_eq!(
        step_name_for("nextest", "stack/rust/root/nextest/default/shard-2-of-4"),
        "Unit and integration tests (shard 2 of 4)"
    );
    assert_eq!(
        step_name_for("fmt", "stack/rust/root/fmt/default"),
        "Format"
    );
}

#[path = "crate_jobs_mbx_tests.rs"]
mod mbx_tests;

#[test]
fn drivers_follow_per_crate_selection() {
    let mut mbx = group("demo", TaskKind::Clippy, &[]);
    mbx.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    let cargo = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![mbx, cargo]),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.drivers["rust-demo"], RenderDriver::Mbx);
    assert_eq!(found.drivers["rust-nested"], RenderDriver::Cargo);
    let steps = names(&found.jobs[0].1);
    assert!(steps.contains(&"Restore MBX objects"), "{steps:?}");
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&"Restore MBX objects"), "{steps:?}");
}

#[test]
fn empty_groups_build_no_jobs() {
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(Vec::new()),
        &ToolCatalog::pinned(),
        &[],
        None,
        2,
    )
    .expect("empty build");
    assert!(found.jobs.is_empty() && found.drivers.is_empty());
}

#[test]
fn acquire_stages_before_report_wrappers() {
    let acquire = Step {
        name: "Acquire Velnor".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: velnor_actions_contract::StepKind::Shell {
            run: vec![String::from("true")],
            env: BTreeMap::new(),
        },
    };
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![group("demo", TaskKind::Clippy, &[])]),
        &ToolCatalog::pinned(),
        &[],
        Some(&acquire),
        2,
    )
    .expect("crate jobs");
    let steps = names(&found.jobs[0].1);
    assert_eq!(
        &steps[..3],
        ["Checkout", "Acquire Velnor", "Download plan"],
        "{steps:?}"
    );
}

/// `Prepare pinned tools` argv of one built job.
fn prepare_run(job: &Job) -> Vec<String> {
    use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;
    let step = job
        .steps
        .iter()
        .find(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("prepare must be a shell step");
    };
    run.clone()
}

#[test]
fn velnor_policy_trims_trio_except_validator_spawning_suites() {
    use velnor_actions_mise::PinnedTool;
    let catalog = ToolCatalog::pinned();
    let trio = [
        catalog.tool_spec(PinnedTool::Actionlint),
        catalog.tool_spec(PinnedTool::Shellcheck),
        catalog.tool_spec(PinnedTool::Zizmor),
    ];
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery(vec![
            group("velnor-actions-orchestrator", TaskKind::Test, &[]),
            group("velnor-actions-cli", TaskKind::Test, &[]),
            group("velnor-actions-contract", TaskKind::Test, &[]),
        ]),
        &catalog,
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 3);
    for (id, job) in &found.jobs {
        let run = prepare_run(job);
        let spawning = id == "rust-velnor-actions-orchestrator" || id == "rust-velnor-actions-cli";
        for spec in &trio {
            assert_eq!(
                run.contains(spec),
                spawning,
                "{id} trio membership follows its executed suite: {run:?}"
            );
        }
        assert!(
            run.contains(&catalog.tool_spec(PinnedTool::Rust)),
            "{id} keeps its driver: {run:?}"
        );
    }
}
