//! Production workflow boundary: one crate job, parallel checks and Required.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{JobConclusion, RequiredJobResult, WorkflowPolicy};
use velnor_actions_rust::TaskKind;

use crate::crate_job_ids::JobOwners;
use crate::crate_jobs::crate_jobs_tests::{discovery, group};

#[test]
fn complete_workflow_runs_one_crate_job_with_parallel_checks() {
    let root = tempfile::tempdir().expect("repository");
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("repository config");
    let config = crate::config::load_config(root.path()).expect("default config");
    let lint = group("demo", TaskKind::Clippy, &[]);
    let build = group("demo", TaskKind::Build, &[]);
    let mut tests = group("demo", TaskKind::Nextest, &[]);
    tests.depends_on.push(build.task_id.clone());
    let doctest = group("demo", TaskKind::Doctest, &[]);
    let docs = group("demo", TaskKind::Doc, &[]);
    let proposals = vec![lint, build, tests, doctest, docs];
    let mut workflow = crate::workflow::build_workflow(
        root.path(),
        &config,
        "main",
        "ubuntu-26.04",
        &discovery(proposals.clone()),
        &[],
    )
    .expect("complete production workflow");
    // Repository workflow assembly stages the locked helper after constructing
    // its graph. Exercise that real attach boundary before final rendering.
    let lock = fixture_generator_lock();
    crate::attach::attach_lock_acquire(
        &mut workflow.ir,
        &lock,
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("stage exact helper in the crate job");
    let owner = "rust-demo";
    let jobs = &workflow.ir.jobs;
    assert_eq!(jobs[owner].needs, ["plan"]);
    assert_eq!(jobs[owner].display_name, "Rust / demo");
    for obsolete in ["rust-demo-lint", "rust-demo-tests", "rust-demo-docs"] {
        assert!(!jobs.contains_key(obsolete), "no per-phase job {obsolete}");
    }
    assert!(jobs["required"].needs.iter().any(|need| need == owner));
    assert_eq!(jobs[owner].timeout_minutes.minutes(), 5);
    let tasks: Vec<_> = jobs[owner]
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
        tasks,
        [
            ("Build test executables", false),
            ("Clippy", true),
            ("Unit and integration tests", true),
            ("Doctests", true),
            ("Documentation", true),
        ]
    );
    let emitted = assert_task_owners(jobs, &proposals);
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir(
        &workflow.ir,
        WorkflowPolicy::VelnorRepositoryV1,
        workflow.support.as_ref(),
        &workflow.context,
    )
    .expect("render the production graph and final fan-in");
    assert!(rendered.contains(owner), "rendered crate job");
    assert_eq!(rendered.matches("background: true").count(), 4);
    assert_eq!(rendered.matches("wait-all:").count(), 1);
    let inventory =
        velnor_actions_contract::NeedsConclusions::from_finalized_jobs("required", jobs)
            .expect("committed Required inventory");
    assert!(inventory.inventory.iter().any(|job| job == owner));
    assert_unsuccessful_owners_fail(&inventory, [owner]);
    assert_eq!(emitted.values().collect::<BTreeSet<_>>().len(), 1);
}

fn fixture_generator_lock() -> velnor_actions_contract::GeneratorLock {
    velnor_actions_contract::GeneratorLock {
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
    }
}

fn assert_unsuccessful_owners_fail(
    inventory: &velnor_actions_contract::NeedsConclusions,
    owners: [&str; 1],
) {
    for owner in owners {
        for conclusion in [
            JobConclusion::Failure,
            JobConclusion::Cancelled,
            JobConclusion::Skipped,
            JobConclusion::Missing,
        ] {
            let results = inventory
                .inventory
                .iter()
                .map(|job_id| RequiredJobResult {
                    job_id: job_id.clone(),
                    conclusion: if job_id == owner {
                        conclusion
                    } else {
                        JobConclusion::Success
                    },
                })
                .collect::<Vec<_>>();
            let mut signals = crate::cover::Signals::default();
            crate::required_dispositions::fold_planned_jobs(
                &[],
                &results,
                &mut signals,
                &mut std::collections::BTreeSet::new(),
            );
            assert!(
                signals.failed || signals.cancelled || signals.not_run,
                "{owner}: {conclusion:?}"
            );
        }
    }
}

fn assert_task_owners(
    jobs: &BTreeMap<String, velnor_actions_contract::Job>,
    proposals: &[velnor_actions_contract::ProposedTask],
) -> BTreeMap<String, String> {
    let mut emitted = BTreeMap::new();
    for (job_id, job) in jobs {
        for step in &job.steps {
            if let velnor_actions_contract::StepKind::TaskExecution { task_id, .. } = &step.kind {
                assert!(emitted.insert(task_id.clone(), job_id.clone()).is_none());
            }
        }
    }
    assert_eq!(
        emitted.len(),
        proposals.len(),
        "no omitted or invented task"
    );
    let owners = JobOwners::new(proposals).expect("owners");
    for task in proposals {
        assert_eq!(
            emitted.get(&task.task_id),
            owners
                .for_member(task, velnor_actions_contract::WorkflowEvent::PullRequest)
                .expect("owner")
                .as_ref(),
            "plan and emitted execution must have the same sole owner"
        );
    }
    emitted
}

#[test]
fn complete_workflow_seed_barrier_preserves_required_and_reports() {
    let root = tempfile::tempdir().expect("repository");
    std::fs::create_dir(root.path().join(".velnor")).expect("config");
    let proposals = [
        TaskKind::Build,
        TaskKind::Clippy,
        TaskKind::Nextest,
        TaskKind::Doc,
    ]
    .into_iter()
    .map(|kind| group("demo", kind, &[]))
    .collect::<Vec<_>>();
    let mut graphs = Vec::new();
    for enabled in [false, true] {
        std::fs::write(root.path().join(".velnor/config.toml"), format!(
            "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n[stacks.rust]\nseed_parallel_targets = {enabled}\n"
        )).expect("config");
        let config = crate::config::load_config(root.path()).expect("load");
        let mut workflow = crate::workflow::build_workflow(
            root.path(),
            &config,
            "main",
            "ubuntu-26.04",
            &discovery(proposals.clone()),
            &[],
        )
        .expect("native graph");
        crate::attach::attach_lock_acquire(
            &mut workflow.ir,
            &fixture_generator_lock(),
            "ubuntu-26.04",
            env!("CARGO_PKG_VERSION"),
        )
        .expect("native acquisition");
        let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir(
            &workflow.ir,
            WorkflowPolicy::VelnorRepositoryV1,
            workflow.support.as_ref(),
            &workflow.context,
        )
        .expect("full rendered workflow");
        assert!(
            rendered.contains("!contains(needs.plan.outputs.not_affected_jobs, ',rust-demo,')")
        );
        assert_eq!(
            rendered.contains("Seed independent parallel targets"),
            enabled
        );
        assert_eq!(rendered.matches("background: true").count(), 3);
        assert_eq!(rendered.matches("wait-all:").count(), 1);
        assert_seed_job_dispositions(&workflow.ir.jobs);
        if enabled {
            let steps = &mut workflow.ir.jobs.get_mut("rust-demo").expect("owner").steps;
            let index = steps
                .iter()
                .position(|step| step.name == "Seed independent parallel targets")
                .expect("barrier");
            steps.remove(index);
        }
        graphs.push(serde_json::to_value(&workflow.ir.jobs).expect("canonical job inventory"));
    }
    assert_eq!(
        graphs[0], graphs[1],
        "Required, reports, acquisition, commands and all remaining job structure stay exact"
    );
}

/// Whole-job selection remains the only skip path when seeding is enabled.
fn assert_seed_job_dispositions(jobs: &BTreeMap<String, velnor_actions_contract::Job>) {
    use velnor_actions_contract::{JobDisposition, PlanJob, StepKind};
    let id = "rust-demo";
    // The orchestrator IR remains policy-neutral here. The rendered workflow
    // above proves that finalization adds the plan gate in either seed mode.
    assert!(jobs[id].condition.is_none());
    assert!(jobs["required"].needs.iter().any(|need| need == id));
    for step in &jobs[id].steps {
        if matches!(step.kind, StepKind::TaskExecution { .. }) {
            assert!(
                step.condition.is_none(),
                "no removed covered-task step gates"
            );
        }
    }
    for (disposition, conclusion, expected) in [
        (JobDisposition::Run, JobConclusion::Success, None),
        (
            JobDisposition::Run,
            JobConclusion::Skipped,
            Some("planned_job_skipped"),
        ),
        (JobDisposition::NotAffected, JobConclusion::Skipped, None),
        (
            JobDisposition::NotAffected,
            JobConclusion::Success,
            Some("gate_mismatch"),
        ),
    ] {
        let mut signals = crate::cover::Signals::default();
        let mut reasons = BTreeSet::new();
        crate::required_dispositions::fold_planned_jobs(
            &[PlanJob {
                job_id: id.to_owned(),
                disposition,
            }],
            &[RequiredJobResult {
                job_id: id.to_owned(),
                conclusion,
            }],
            &mut signals,
            &mut reasons,
        );
        assert_eq!(
            signals.failed || signals.cancelled || signals.not_run || signals.planning_failed,
            expected.is_some(),
            "{disposition:?}: {conclusion:?}"
        );
        assert_eq!(reasons, expected.into_iter().map(str::to_owned).collect());
    }
}
