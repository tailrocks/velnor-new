//! Opt-in seeding preserves the sequential producer and parallel quality gates.
use super::*;

fn seeded(tasks: Vec<ProposedTask>, enabled: bool, roots: &[String]) -> CrateBuild {
    build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(tasks),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: roots,
        acquire: None,
        max_parallel_jobs: 2,
        timeout: velnor_actions_contract::JobTimeout::CRATE,
        helper_version: env!("CARGO_PKG_VERSION"),
        seed_roots: &[],
        seed_parallel_targets: enabled,
        crates: &BTreeMap::from([(
            "demo".to_owned(),
            velnor_actions_contract::RustCrateConfig { build_jobs: 1 },
        )]),
    })
    .expect("crate jobs")
}

#[test]
fn seed_barrier_is_opt_in_after_build_before_every_parallel_check() {
    let tasks: Vec<_> = [
        TaskKind::Build,
        TaskKind::Fmt,
        TaskKind::Clippy,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
    ]
    .into_iter()
    .map(|kind| group("demo", kind, &[]))
    .collect();
    let ordinary = seeded(tasks.clone(), false, &[]);
    let enabled = seeded(tasks, true, &[]);
    let original = &ordinary.jobs[0].1;
    let prepared = &enabled.jobs[0].1;
    for step in &prepared.steps {
        if let velnor_actions_contract::StepKind::TaskExecution { env, .. } = &step.kind {
            assert_eq!(env.get("CARGO_BUILD_JOBS").map(String::as_str), Some("1"));
        }
    }
    assert_eq!(original.needs, prepared.needs);
    assert_eq!(original.timeout_minutes, prepared.timeout_minutes);
    let index = prepared
        .steps
        .iter()
        .position(|step| step.name == "Seed independent parallel targets")
        .expect("one barrier");
    assert_eq!(prepared.steps[index - 1].name, "Build test executables");
    assert!(prepared.steps[index + 1].parallel);
    assert!(!prepared.steps[index].parallel);
    let mut without_seed = prepared.steps.clone();
    without_seed.remove(index);
    assert_eq!(
        without_seed, original.steps,
        "all other steps retain exact commands, env, order and ownership"
    );
    assert_eq!(
        prepared
            .steps
            .iter()
            .filter(|step| step.name == "Seed independent parallel targets")
            .count(),
        1
    );
}

#[test]
fn seed_barrier_uses_exact_scoped_lanes_and_never_invents_a_producer() {
    let path = "crates/work/members/demo/Cargo.toml";
    let tasks = [TaskKind::Build, TaskKind::Clippy, TaskKind::Doc]
        .into_iter()
        .map(|kind| {
            let mut task = group("demo", kind, &[]);
            task.identity.unit_path = path.to_owned();
            task
        })
        .collect();
    let enabled = seeded(tasks, true, &["crates/work".to_owned()]);
    let step = enabled.jobs[0]
        .1
        .steps
        .iter()
        .find(|step| step.name == "Seed independent parallel targets")
        .expect("seed");
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("shell barrier")
    };
    assert!(
        run.last()
            .expect("script")
            .ends_with("crates/work/target velnor-clippy velnor-doc")
    );
    for kinds in [
        vec![TaskKind::Clippy, TaskKind::Doc],
        vec![TaskKind::Build, TaskKind::Clippy],
        vec![TaskKind::Build, TaskKind::Nextest, TaskKind::Doctest],
    ] {
        let tasks = kinds
            .into_iter()
            .map(|kind| group("demo", kind, &[]))
            .collect();
        let jobs = seeded(tasks, true, &[]);
        assert!(
            jobs.jobs[0]
                .1
                .steps
                .iter()
                .all(|step| step.name != "Seed independent parallel targets")
        );
    }
}
