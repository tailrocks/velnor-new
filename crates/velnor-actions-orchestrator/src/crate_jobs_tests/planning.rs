use super::*;
use crate::crate_job_ids::job_id_for_member;

#[test]
fn groups_obligations_into_one_ordered_job_per_crate() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let doctest = group("demo", TaskKind::Doctest, &[clippy_id.as_str()]);
    let doc = group("demo", TaskKind::Doc, &[clippy_id.as_str()]);
    let other = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![test, doc, doctest, clippy, other]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
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
fn qualification_crate_jobs_keep_one_matrix_entry_each() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let test = group("demo", TaskKind::Test, &[clippy.task_id.as_str()]);
    let proposals = vec![clippy, test];
    let discovery = discovery(proposals.clone());
    let normal = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery,
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("normal grouped jobs");
    let qualification = build_qualification_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery,
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("qualification jobs");

    assert_eq!(normal.jobs.len(), 1);
    assert_eq!(normal.jobs[0].0, "rust-demo");
    assert_eq!(qualification.jobs.len(), 2);
    let mut expected_ids: Vec<String> = proposals
        .iter()
        .map(|task| qualification_job_id_for_member(task).expect("qualification ID"))
        .collect();
    expected_ids.sort();
    assert_eq!(
        qualification
            .jobs
            .iter()
            .map(|(job_id, _)| job_id.clone())
            .collect::<Vec<_>>(),
        expected_ids
    );
    for (job_id, job) in &qualification.jobs {
        let model_step_count = job
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    &step.kind,
                    velnor_actions_contract::StepKind::TaskExecution { .. }
                )
            })
            .count();
        assert_eq!(model_step_count, 1, "{job_id}");
        assert!(job.needs.contains(&PLAN_JOB_ID.to_owned()));
    }
}

#[test]
fn qualification_plan_binding_uses_the_per_entry_job_id() {
    let task = group("demo", TaskKind::Clippy, &[]);
    let expected = qualification_job_id_for_member(&task).expect("qualification ID");
    assert_eq!(
        job_id_for_member(
            std::slice::from_ref(&task),
            &task,
            velnor_actions_contract::WorkflowEvent::Qualification,
        ),
        Some(expected)
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
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![testless, workspace_fmt, clippy]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
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
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(groups.clone()),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    })
    .expect("crate jobs");
    assert_eq!(found.jobs.len(), 1);
    let built = found.jobs[0].0.as_str();
    assert_eq!(
        job_id_for_member(
            &groups,
            &groups[0],
            velnor_actions_contract::WorkflowEvent::Push
        )
        .as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(
            &groups,
            &groups[1],
            velnor_actions_contract::WorkflowEvent::Push
        )
        .as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(
            &groups,
            &groups[2],
            velnor_actions_contract::WorkflowEvent::Push
        )
        .as_deref(),
        Some(PLAN_JOB_ID)
    );
    let outsider = group("other", TaskKind::Clippy, &[]);
    assert_eq!(
        job_id_for_member(
            &groups,
            &outsider,
            velnor_actions_contract::WorkflowEvent::Push
        ),
        None
    );
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
