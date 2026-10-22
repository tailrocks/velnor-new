use super::*;
use crate::crate_job_ids::job_id_for_member;
#[path = "planning/qualification_order.rs"]
mod qualification_order;
use qualification_order::assert_grouped_qualification_steps;

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
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
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
fn duplicate_rust_display_names_include_manifest_context() {
    let mut first = group("alpha", TaskKind::Clippy, &[]);
    first.display_name = "shared".to_owned();
    first.identity.unit_path = "workspace-a/crates/shared/Cargo.toml".to_owned();
    let mut second = group("beta", TaskKind::Clippy, &[]);
    second.display_name = "shared".to_owned();
    second.identity.unit_path = "workspace-b/crates/shared/Cargo.toml".to_owned();
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![first, second]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("same-name crate jobs");
    let names: Vec<&str> = found
        .jobs
        .iter()
        .map(|(_, job)| job.display_name.as_str())
        .collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|name| name.contains("Cargo.toml")));
}

#[test]
fn rust_qualification_reuses_one_job_for_all_configurations_and_tasks() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let mut test = group("demo", TaskKind::Test, &[clippy.task_id.as_str()]);
    test.task_id = test.task_id.replace("/default", "/all-features");
    test.configuration = "all-features".to_owned();
    let proposals = vec![clippy, test];
    let discovery = discovery(proposals.clone());
    let input = CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery,
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    };
    let normal = build_crate_jobs(input).expect("normal grouped jobs");
    let qualified = build_crate_jobs_inner(
        input,
        true,
        None,
        velnor_actions_contract::JobTimeout::new(
            velnor_actions_contract::config::default_dependency_seed_timeout_minutes(),
        )
        .expect("valid default dependency seed timeout"),
    )
    .expect("shared qualification jobs");
    let qualification = build_qualification_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery,
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("qualification jobs");

    assert_eq!(normal.jobs.len(), 1);
    assert_eq!(normal.jobs[0].0, "rust-demo");
    assert!(
        normal.jobs[0].1.steps.iter().all(
            |step| step.name != crate::qualification_source_evidence::SOURCE_EVIDENCE_STEP_NAME
        )
    );
    assert!(
        qualification.jobs.is_empty(),
        "Rust has no duplicate executor jobs"
    );
    assert_eq!(qualified.jobs.len(), 1);
    assert_eq!(qualified.jobs[0].0, "rust-demo");
    assert_grouped_qualification_steps(&qualified.jobs[0].1);
}

#[test]
fn qualification_plan_binding_uses_the_per_entry_job_id() {
    let task = group("demo", TaskKind::Clippy, &[]);
    assert_eq!(
        job_id_for_member(
            std::slice::from_ref(&task),
            &task,
            velnor_actions_contract::WorkflowEvent::Qualification,
        ),
        Ok(Some("rust-demo".to_owned()))
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
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
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
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
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
        .expect("owner")
        .as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(
            &groups,
            &groups[1],
            velnor_actions_contract::WorkflowEvent::Push
        )
        .expect("owner")
        .as_deref(),
        Some(built)
    );
    assert_eq!(
        job_id_for_member(
            &groups,
            &groups[2],
            velnor_actions_contract::WorkflowEvent::Push
        )
        .expect("owner")
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
        Ok(None)
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
