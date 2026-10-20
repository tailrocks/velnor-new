use velnor_actions_contract::workflow::{Job, StepRole};

pub(super) fn assert_grouped_qualification_steps(job: &Job) {
    assert_setup_and_identity_order(job);
    assert_task_gate_and_position(job);
}

fn positions(job: &Job, predicate: fn(StepRole) -> bool) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| step.role.filter(|role| predicate(*role)).map(|_| index))
        .collect()
}

fn assert_setup_and_identity_order(job: &Job) {
    use StepRole as Role;
    let setup = positions(job, |role| {
        matches!(role, Role::PreparePinnedTools | Role::PrepareRustComponents)
    });
    let before = positions(job, |role| role == Role::QualificationRuntimeIdentity);
    let after = positions(job, |role| {
        role == Role::QualificationCacheRuntimeIdentityAfterImport
    });
    let imports = positions(job, |role| {
        matches!(
            role,
            Role::QualificationCacheImport | Role::QualificationCachePayloadImport
        )
    });
    assert!(!setup.is_empty(), "pinned tool setup is present");
    assert!(
        !before.is_empty(),
        "runtime identity is checked before imports"
    );
    assert!(
        !after.is_empty(),
        "runtime identity is checked after imports"
    );
    assert!(!imports.is_empty(), "the job has cache imports");

    let first_identity = before
        .iter()
        .chain(&after)
        .min()
        .copied()
        .expect("runtime identity step");
    let last_import = imports.iter().max().copied().expect("cache import");
    let first_after = after.iter().min().copied().expect("post-import identity");
    assert!(setup.iter().all(|step| *step < first_identity));
    assert!(before.iter().all(|step| *step < last_import));
    assert!(last_import < first_after);

    assert!(
        job.steps
            .iter()
            .filter(|step| {
                step.role.is_some_and(|role| {
                    matches!(
                        role,
                        Role::PreparePinnedTools
                            | Role::PrepareRustComponents
                            | Role::QualificationRuntimeIdentity
                            | Role::QualificationCacheRuntimeIdentityAfterImport
                            | Role::QualificationCacheImport
                            | Role::QualificationCachePayloadImport
                    )
                })
            })
            .all(|step| !step.parallel),
        "setup, identity probes, and imports remain sequential"
    );
}

fn assert_task_gate_and_position(job: &Job) {
    let execution_steps: Vec<_> = job
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step.kind,
                velnor_actions_contract::StepKind::TaskExecution { .. }
            )
        })
        .collect();
    assert_eq!(
        execution_steps.len(),
        2,
        "both task obligations stay in one job"
    );

    assert!(
        execution_steps.iter().all(|step| step.condition.is_none()),
        "task-execution authority remains unconditional"
    );

    let last_identity = positions(job, |role| {
        role == velnor_actions_contract::workflow::StepRole::QualificationCacheRuntimeIdentityAfterImport
    })
    .into_iter()
    .max()
    .expect("post-import identity checks");
    assert!(job.steps.iter().enumerate().all(|(index, step)| {
        !(step.parallel
            || matches!(
                step.kind,
                velnor_actions_contract::StepKind::TaskExecution { .. }
            ))
            || index > last_identity
    }));

    let evidence = job
        .steps
        .iter()
        .find(|step| step.name == crate::qualification_source_evidence::SOURCE_EVIDENCE_STEP_NAME)
        .expect("one aggregated source evidence step");
    assert_eq!(
        evidence.condition.as_deref(),
        Some("needs.plan.outputs.qualification_campaign != ''")
    );
}

#[test]
fn qualified_multi_configuration_plan_renders_with_task_authority_intact() {
    use velnor_actions_contract::WorkflowPolicy;

    let root = tempfile::tempdir().expect("repository");
    write_qualification_config(root.path());
    let config = crate::config::load_config(root.path()).expect("config");
    let mut workflow = build_qualified_fixture(root.path(), &config);
    attach_fixture_lock(&mut workflow);

    let job = workflow
        .ir
        .jobs
        .get("rust-demo")
        .expect("grouped crate job");
    assert_grouped_qualification_steps(job);
    assert_eq!(
        task_execution_count(job),
        2,
        "default and all-features tasks share one crate job"
    );
    let nested_job = workflow
        .ir
        .jobs
        .get("rust-nested")
        .expect("second crate job");
    assert_setup_and_identity_order(nested_job);
    for job in [job, nested_job] {
        let last_identity = positions(job, |role| {
            role == StepRole::QualificationCacheRuntimeIdentityAfterImport
        })
        .into_iter()
        .max()
        .expect("post-import identity checks");
        let tasks: Vec<_> = job
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                matches!(
                    step.kind,
                    velnor_actions_contract::StepKind::TaskExecution { .. }
                )
            })
            .collect();
        assert!(!tasks.is_empty(), "each crate job has task execution");
        assert!(
            tasks
                .iter()
                .all(|(index, step)| *index > last_identity && step.condition.is_none())
        );
        assert!(tasks.iter().any(|(_, step)| step.parallel));
    }
    velnor_actions_workflow_renderer::render::render_workflow_ir(
        &workflow.ir,
        WorkflowPolicy::VelnorRepositoryV1,
        workflow.support.as_ref(),
        &workflow.context,
    )
    .expect("rendered workflow validates task authority");
}

fn write_qualification_config(root: &std::path::Path) {
    std::fs::create_dir(root.join(".velnor")).expect("config directory");
    std::fs::write(
        root.join(".velnor/config.toml"),
        r#"
schema = 2
[workflow]
name = "CI"
policy = "velnor-repository-v1"
[[stacks.rust.configurations]]
name = "default"
features = ["default"]
target = "host"
[[stacks.rust.configurations]]
name = "all-features"
features = []
target = "host"
[execution]
default_profile = "hosted"
mode = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
workflows = ["qualification"]
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#,
    )
    .expect("workflow config");
}

fn build_qualified_fixture(
    root: &std::path::Path,
    config: &velnor_actions_contract::VelnorConfig,
) -> crate::workflow::WorkflowPlan {
    use velnor_actions_rust::{CompileDriver, TaskKind};

    let mut clippy = super::super::group("demo", TaskKind::Clippy, &[]);
    clippy.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    let mut test = super::super::group("demo", TaskKind::Test, &[]);
    test.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    test.task_id = test.task_id.replace("/default", "/all-features");
    test.configuration = "all-features".to_owned();
    let nested = super::super::group("nested", TaskKind::Clippy, &[]);
    let found = super::super::discovery(vec![clippy, test, nested]);
    crate::workflow::build_workflow(root, config, "main", "ubuntu-26.04", &found, &[])
        .expect("full qualified workflow plan")
}

fn attach_fixture_lock(workflow: &mut crate::workflow::WorkflowPlan) {
    let lock = velnor_actions_contract::GeneratorLock {
        schema: 1,
        generator: velnor_actions_contract::LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: "a".repeat(40),
            binaries: vec![velnor_actions_contract::GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/fixture-helper".to_owned(),
                sha256: "b".repeat(64),
            }],
        },
        mise_bootstrap: velnor_actions_contract::MiseBootstrap {
            version: velnor_actions_mise::MISE_VERSION.to_owned(),
            artifact: "https://example.invalid/fixture-mise".to_owned(),
            sha256: "c".repeat(64),
        },
        actions: Vec::new(),
    };
    crate::attach::attach_lock_acquire(
        &mut workflow.ir,
        &lock,
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("attach exact helper before render");
}

fn task_execution_count(job: &velnor_actions_contract::workflow::Job) -> usize {
    job.steps
        .iter()
        .filter(|step| {
            matches!(
                step.kind,
                velnor_actions_contract::StepKind::TaskExecution { .. }
            )
        })
        .count()
}
