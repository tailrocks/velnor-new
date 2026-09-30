//! Crate-job construction tests.
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;

/// Runnable fixture group for one package/kind pair.
fn group(package: &str, kind: TaskKind, gated_by: &[&str]) -> TaskGroup {
    let key = if package == "demo" { "root" } else { package };
    TaskGroup {
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
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
        nextest_profile: "default".to_owned(),
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

/// Discovery shell carrying only task groups.
fn discovery(groups: Vec<TaskGroup>) -> Discovery {
    Discovery {
        statuses: Vec::new(),
        workspaces: Vec::new(),
        task_groups: groups,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
    }
}

/// Step names of one built job.
fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

#[test]
fn groups_obligations_into_one_ordered_job_per_crate() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let doc = group("demo", TaskKind::Doc, &[clippy_id.as_str()]);
    let other = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![test, doc, clippy, other]),
        &ToolCatalog::pinned(),
        &[],
        &[],
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
    let (Some(checkout), Some(prepare), Some(components), Some(lint), Some(run), Some(docs)) = (
        at("Checkout"),
        at("Prepare pinned tools"),
        at("Prepare Rust components"),
        at("Clippy"),
        at("Unit and integration tests"),
        at("Documentation"),
    ) else {
        panic!("crate steps out of shape: {steps:?}");
    };
    assert!(checkout < prepare && prepare < components, "{steps:?}");
    assert!(components < lint && lint < run && run < docs, "{steps:?}");
}

#[test]
fn skips_testless_and_workspace_groups() {
    let mut testless = group("demo", TaskKind::Doctest, &[]);
    testless.no_test_targets = true;
    let mut workspace_fmt = group("demo", TaskKind::Fmt, &[]);
    workspace_fmt.package_id.clear();
    workspace_fmt.package_name.clear();
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![testless, workspace_fmt, clippy]),
        &ToolCatalog::pinned(),
        &[],
        &[],
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let steps = names(&found.jobs[0].1);
    assert!(steps.contains(&"Clippy"), "{steps:?}");
    assert!(!steps.contains(&"Doctests"), "{steps:?}");
    assert!(!steps.contains(&"Format"), "{steps:?}");
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
        step_name_for(
            TaskKind::Nextest,
            "stack/rust/root/nextest/default/shard-2-of-4"
        ),
        "Unit and integration tests (shard 2 of 4)"
    );
    assert_eq!(
        step_name_for(TaskKind::Fmt, "stack/rust/root/fmt/default"),
        "Format"
    );
}

#[test]
fn drivers_follow_per_crate_selection() {
    let mut mbx = group("demo", TaskKind::Clippy, &[]);
    mbx.compile_driver = "mbx".to_owned();
    let cargo = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![mbx, cargo]),
        &ToolCatalog::pinned(),
        &[],
        &[],
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
        &discovery(Vec::new()),
        &ToolCatalog::pinned(),
        &[],
        &[],
    )
    .expect("empty build");
    assert!(found.jobs.is_empty() && found.drivers.is_empty());
}

#[test]
fn allowlisted_custom_tasks_append_after_obligations() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let allowlist = vec!["audit".to_owned()];
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![clippy]),
        &ToolCatalog::pinned(),
        &[],
        &allowlist,
    )
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let steps = names(&found.jobs[0].1);
    let lint = steps.iter().position(|seen| *seen == "Clippy");
    let custom = steps.iter().position(|seen| *seen == "Custom task audit");
    assert!(lint < custom, "{steps:?}");
    let joined: Vec<String> = found.jobs[0]
        .1
        .steps
        .iter()
        .map(|step| format!("{} {:?}", step.name, step.kind))
        .collect();
    assert!(
        !joined.join("\n").contains("undeclared-task"),
        "undeclared names never emitted: {joined:?}"
    );
}
