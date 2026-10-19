//! One crate job: sequential build, then parallel checks in target lanes.

use super::*;

#[test]
fn one_job_per_crate_builds_first_then_runs_checks_in_parallel() {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let fmt = group("demo", TaskKind::Fmt, &[]);
    let build = group("demo", TaskKind::Build, &[]);
    let mut nextest = group("demo", TaskKind::Nextest, &[]);
    nextest.depends_on.push(build.task_id.clone());
    let doctest = group("demo", TaskKind::Doctest, &[]);
    let doc = group("demo", TaskKind::Doc, &[]);
    let other = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![nextest, doc, doctest, clippy, fmt, build, other]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    let ids: Vec<&str> = found.jobs.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["rust-demo", "rust-nested"]);
    let jobs: BTreeMap<&str, &Job> = found
        .jobs
        .iter()
        .map(|(id, job)| (id.as_str(), job))
        .collect();
    assert_eq!(jobs["rust-demo"].needs, [PLAN_JOB_ID]);
    super::preludes::assert_crate_preludes(&found.jobs);
    assert_eq!(jobs["rust-demo"].display_name, "Rust / demo");
    let obligations: Vec<(&str, bool)> = jobs["rust-demo"]
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step.kind,
                velnor_actions_contract::StepKind::TaskExecution { .. }
            )
        })
        .map(|step| (step.name.as_str(), step.parallel))
        .collect();
    assert_eq!(
        obligations,
        [
            ("Build test executables", false),
            ("Format", true),
            ("Clippy", true),
            ("Unit and integration tests", true),
            ("Doctests", true),
            ("Documentation", true),
        ]
    );
    assert_target_lanes(jobs["rust-demo"]);
    let tail = names(jobs["rust-demo"]);
    assert_eq!(tail.last(), Some(&"Upload crate reports"));
    let single = names(jobs["rust-nested"]);
    assert!(single.contains(&"Clippy"));
    assert!(
        jobs["rust-nested"].steps.iter().any(|step| step.parallel),
        "the check uses native parallel execution"
    );
}

#[test]
fn nested_workspace_lanes_live_below_that_workspace_target() {
    let mut clippy = group("runner", TaskKind::Clippy, &[]);
    clippy.identity.unit_path = "crates/velnor-runner/crates/runner/Cargo.toml".to_owned();
    let mut doc = group("runner", TaskKind::Doc, &[]);
    doc.identity.unit_path = clippy.identity.unit_path.clone();
    let roots = ["crates/velnor-runner".to_owned()];
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![clippy, doc]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &roots,
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        crates: &std::collections::BTreeMap::new(),
    })
    .expect("crate jobs");
    let dirs: BTreeSet<&str> = found.jobs[0]
        .1
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            velnor_actions_contract::StepKind::TaskExecution { env, .. } => {
                env.get("CARGO_TARGET_DIR").map(String::as_str)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        dirs,
        BTreeSet::from(["target/velnor-clippy", "target/velnor-doc"])
    );
}

/// Clippy and documentation use their own lanes; the rest share `target/`.
fn assert_target_lanes(job: &Job) {
    let lanes: BTreeMap<&str, Option<&str>> = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            velnor_actions_contract::StepKind::TaskExecution { env, .. } => Some((
                step.name.as_str(),
                env.get("CARGO_TARGET_DIR").map(String::as_str),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(lanes["Clippy"], Some("target/velnor-clippy"));
    assert_eq!(lanes["Documentation"], Some("target/velnor-doc"));
    for shared in [
        "Build test executables",
        "Format",
        "Unit and integration tests",
        "Doctests",
    ] {
        assert_eq!(lanes[shared], None, "{shared} uses the workspace target");
    }
}

/// `CARGO_BUILD_JOBS` of every task step in one crate job, by step name.
fn build_jobs_by_step(job: &Job) -> BTreeMap<&str, Option<&str>> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            velnor_actions_contract::StepKind::TaskExecution { env, .. } => Some((
                step.name.as_str(),
                env.get("CARGO_BUILD_JOBS").map(String::as_str),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn configured_build_jobs_cap_every_cargo_step_of_that_crate_only() {
    let tasks = vec![
        group("demo", TaskKind::Build, &[]),
        group("demo", TaskKind::Clippy, &[]),
        group("demo", TaskKind::Doc, &[]),
        group("demo", TaskKind::Doctest, &[]),
        group("other", TaskKind::Clippy, &[]),
        group("other", TaskKind::Doc, &[]),
    ];
    let crates = BTreeMap::from([(
        "demo".to_owned(),
        velnor_actions_contract::RustCrateConfig { build_jobs: 1 },
    )]);
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        crates: &crates,
    })
    .expect("crate jobs");
    let jobs: BTreeMap<&str, &Job> = found
        .jobs
        .iter()
        .map(|(id, job)| (id.as_str(), job))
        .collect();
    let capped = build_jobs_by_step(jobs["rust-demo"]);
    assert_eq!(capped["Build test executables"], Some("1"));
    for name in ["Clippy", "Documentation", "Doctests"] {
        assert_eq!(capped[name], Some("1"), "{name} is capped");
    }
    let uncapped = build_jobs_by_step(jobs["rust-other"]);
    assert!(
        uncapped.values().all(Option::is_none),
        "other crates keep Cargo's default: {uncapped:?}"
    );
}

#[test]
fn build_jobs_for_an_unknown_package_fail_closed() {
    let crates = BTreeMap::from([(
        "typo".to_owned(),
        velnor_actions_contract::RustCrateConfig { build_jobs: 1 },
    )]);
    let err = reject_unknown_crates(
        &crates,
        &discovery(vec![group("demo", TaskKind::Clippy, &[])]),
    )
    .expect_err("unknown package");
    assert!(err.to_string().contains("unknown_rust_package"), "{err}");
    reject_unknown_crates(
        &BTreeMap::from([(
            "demo".to_owned(),
            velnor_actions_contract::RustCrateConfig { build_jobs: 1 },
        )]),
        &discovery(vec![group("demo", TaskKind::Clippy, &[])]),
    )
    .expect("known package");
}
