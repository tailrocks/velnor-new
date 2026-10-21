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

fn build_mbx(nextest: bool, fetch_roots: &[String]) -> CrateBuild {
    let mut mbx = group("demo", TaskKind::Clippy, &[]);
    mbx.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    if nextest {
        mbx.identity.test_runner = "cargo_nextest".to_owned();
    }
    build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![mbx, group("nested", TaskKind::Clippy, &[])]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots,
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_parallel_targets: false,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs")
}

#[test]
fn an_mbx_workspace_gets_one_trusted_seed_job_that_is_the_only_bundle_writer() {
    use velnor_actions_contract::StepRole;
    let found = build_mbx(true, &[]);
    let [(id, seed)] = found.seeds.as_slice() else {
        panic!("one seed for the root workspace");
    };
    assert_eq!(id, "dependency-seed");
    assert_eq!(found.drivers[id], RenderDriver::Mbx);
    assert_eq!(seed.needs, ["plan"]);
    assert_eq!(
        seed.condition.as_deref(),
        Some(velnor_actions_contract::workflow::DEPENDENCY_SEED_JOB_CONDITION),
        "only a protected default-branch push seeds"
    );
    let roles: Vec<_> = seed.steps.iter().map(|step| step.role).collect();
    let lanes: Vec<_> = seed
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MbxSeedBuild))
        .map(|step| step.name.as_str())
        .collect();
    assert_eq!(
        lanes,
        [
            "Compile test executables (seed)",
            "Compile Clippy lane (seed)",
            "Compile documentation lane (seed)"
        ]
    );
    let build_at = roles
        .iter()
        .position(|role| *role == Some(StepRole::MbxSeedBuild))
        .expect("lane builds");
    let mark_at = roles
        .iter()
        .position(|role| *role == Some(StepRole::MbxProducerMark))
        .expect("producer mark");
    let restore_at = roles
        .iter()
        .position(|role| *role == Some(StepRole::MbxBundleRestore))
        .expect("seed restores the previous bundle");
    assert!(restore_at < build_at && build_at < mark_at, "{roles:?}");
    for step in seed
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::MbxSeedBuild))
    {
        let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
            panic!("lane builds are shell steps");
        };
        let command = run.join(" ");
        assert!(command.contains("--workspace") && command.contains("--locked --offline"));
    }
    let crate_job = &found.jobs[0].1;
    let crate_roles: Vec<_> = crate_job.steps.iter().map(|step| step.role).collect();
    assert!(crate_roles.contains(&Some(StepRole::MbxBundleRestore)));
    assert!(
        !crate_roles.contains(&Some(StepRole::MbxProducerMark)),
        "crate jobs restore the seed read-only and never mark a producer"
    );
}

#[test]
fn each_nested_mbx_workspace_gets_its_own_seed_and_cargo_jobs_get_none() {
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
    let ids: Vec<_> = found.seeds.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["dependency-seed-crates-inner"]);
    assert_eq!(
        found.seeds[0].1.display_name,
        "Dependency seed / crates/inner"
    );
    let cargo_only = build_crate_jobs(CrateJobInputs {
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
    assert!(cargo_only.seeds.is_empty());
}
