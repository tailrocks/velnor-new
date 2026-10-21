use super::*;

#[test]
fn drivers_follow_per_crate_selection() {
    let mut mbx = group("demo", TaskKind::Clippy, &[]);
    mbx.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    let cargo = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![mbx, cargo]),
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
    assert_eq!(found.drivers["rust-demo"], RenderDriver::Mbx);
    assert_eq!(found.drivers["rust-nested"], RenderDriver::Cargo);
    let steps = names(&found.jobs[0].1);
    let preflight = steps
        .iter()
        .position(|name| *name == "Verify Rust before MBX action")
        .expect("MBX preflight");
    let restore = steps
        .iter()
        .position(|name| *name == "Set up MBX")
        .expect("MBX restore");
    let version_check = steps
        .iter()
        .position(|name| *name == "Verify native MBX version")
        .expect("exact native MBX version check");
    assert!(
        preflight < restore && restore < version_check,
        "Rust is checked before the action and MBX is checked after it: {steps:?}"
    );
    assert_eq!(
        steps
            .iter()
            .filter(|name| **name == "Verify Rust before MBX action")
            .count(),
        1,
        "every MBX action has one strict preflight"
    );
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&"Set up MBX"), "{steps:?}");
    assert!(
        !steps.contains(&"Verify Rust before MBX action"),
        "Cargo jobs have no MBX preflight: {steps:?}"
    );
}

#[test]
fn mbx_crates_read_seed_and_seed_jobs_are_the_only_bundle_writers() {
    use velnor_actions_contract::{StepKind, StepRole};

    let mut task = group("demo", TaskKind::Clippy, &[]);
    task.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    task.identity.test_runner = "cargo_nextest".to_owned();
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![task]),
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
    assert_eq!(found.seeds.len(), 1);
    let (seed_id, seed) = &found.seeds[0];
    assert_eq!(seed_id, "dependency-seed");
    assert_eq!(seed.needs, [PLAN_JOB_ID]);
    assert_eq!(
        seed.condition.as_deref(),
        Some(crate::crate_jobs::dependency_seed::SEED_JOB_CONDITION)
    );
    let roles: Vec<_> = seed.steps.iter().map(|step| step.role).collect();
    assert_eq!(
        roles
            .iter()
            .filter(|role| **role == Some(StepRole::MbxSeedBuild))
            .count(),
        3,
        "The seed must have test, Clippy, and documentation lanes."
    );
    assert_eq!(roles.last(), Some(&Some(StepRole::MbxProducerMark)));
    for lane in seed
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MbxSeedBuild))
    {
        let StepKind::Shell { run, .. } = &lane.kind else {
            panic!("Seed lanes must use shell commands.");
        };
        let command = run.join(" ");
        assert!(command.contains("--workspace"));
        assert!(command.contains("--locked --offline"));
    }
    let crate_job = &found.jobs[0].1;
    let crate_roles: Vec<_> = crate_job.steps.iter().map(|step| step.role).collect();
    assert!(crate_roles.contains(&Some(StepRole::MbxBundleRestore)));
    assert!(!crate_roles.contains(&Some(StepRole::MbxProducerMark)));
}

#[test]
fn nested_mbx_workspace_has_a_seed_but_cargo_jobs_do_not() {
    let mut nested = group("nested", TaskKind::Clippy, &[]);
    nested.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    nested.identity.unit_path = "crates/inner/Cargo.toml".to_owned();
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![nested]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &["crates/inner".to_owned()],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    assert_eq!(
        found
            .seeds
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["dependency-seed-crates-inner"]
    );
    assert_eq!(
        found.seeds[0].1.display_name,
        "Dependency seed / crates/inner"
    );

    let cargo = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![group("plain", TaskKind::Clippy, &[])]),
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
    assert!(cargo.seeds.is_empty());
}

#[test]
fn colliding_flattened_workspace_roots_get_distinct_seed_ids() {
    let mut dashed = group("dashed", TaskKind::Clippy, &[]);
    dashed.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    dashed.identity.unit_path = "crates/a-b/Cargo.toml".to_owned();
    let mut nested = group("nested", TaskKind::Clippy, &[]);
    nested.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    nested.identity.unit_path = "crates/a/b/Cargo.toml".to_owned();

    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![dashed, nested]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &["crates/a-b".to_owned(), "crates/a/b".to_owned()],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");

    let mut seed_ids: Vec<_> = found.seeds.iter().map(|(id, _)| id.as_str()).collect();
    seed_ids.sort_unstable();
    assert_eq!(
        seed_ids,
        [
            "dependency-seed-6372617465732f612d62",
            "dependency-seed-6372617465732f612f62",
        ]
    );
}

#[test]
fn accepted_workspace_roots_get_valid_injective_seed_ids() {
    use super::dependency_seed::{SeedWorkspace, unique_seed_job_ids};

    let roots = [
        "crates/a-b",
        "crates/a/b",
        "crates/lib.v2",
        "crates/lib_2",
        "crates/lib space",
        "crates/velnor",
        "crates/é",
    ];
    for root in roots {
        velnor_actions_contract::validate_fetch_root(root).expect("The root must remain valid.");
    }
    let workspaces: Vec<_> = roots
        .iter()
        .map(|root| SeedWorkspace {
            root: (*root).to_owned(),
            nextest: false,
        })
        .collect();
    let ids = unique_seed_job_ids(&workspaces);
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        roots.len()
    );
    for id in &ids {
        velnor_actions_contract::validate_job_id(id).expect("The seed job ID must be valid.");
    }

    let ordinary = [SeedWorkspace {
        root: "crates/lib_2".to_owned(),
        nextest: false,
    }];
    assert_eq!(
        unique_seed_job_ids(&ordinary),
        ["dependency-seed-crates-lib_2"]
    );
}
