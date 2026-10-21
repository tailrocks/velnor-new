//! Compose actual seed construction, contract validation and final YAML emission.
use super::fixture_generator_lock;
use crate::crate_jobs::crate_jobs_tests::{discovery, group};
use std::collections::BTreeSet;
use velnor_actions_contract::{PermissionLevel, Permissions, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_rust::TaskKind;
use velnor_actions_workflow_renderer::render::render_workflow_ir;

fn workflow() -> crate::workflow::WorkflowPlan {
    let root = tempfile::tempdir().expect("repository");
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("config");
    let mut proposals = Vec::new();
    for (package, manifest) in [("demo", "Cargo.toml"), ("nested", "nested/Cargo.toml")] {
        let mut task = group(package, TaskKind::Clippy, &[]);
        task.identity.compile_driver = "mbx".to_owned();
        task.identity.test_runner = "cargo_nextest".to_owned();
        task.identity.unit_path = manifest.to_owned();
        proposals.push(task);
    }
    let mut workflow = crate::workflow::build_workflow(
        root.path(),
        &config,
        "testmain",
        "ubuntu-26.04",
        &discovery(proposals),
        &["nested".to_owned()],
    )
    .expect("production workflow");
    crate::attach::attach_lock_acquire(
        &mut workflow.ir,
        &fixture_generator_lock(),
        "ubuntu-26.04",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("stage exact helper");
    workflow
}

fn render(workflow: &crate::workflow::WorkflowPlan) -> Result<String, crate::OrchestratorError> {
    render_workflow_ir(
        &workflow.ir,
        WorkflowPolicy::VelnorRepositoryV1,
        workflow.support.as_ref(),
        &workflow.context,
    )
    .map_err(Into::into)
}

fn optional_jobs(yaml: &str) -> BTreeSet<String> {
    let mut job = None;
    let mut found = BTreeSet::new();
    for line in yaml.lines() {
        if let Some(header) = line.strip_prefix("  ")
            && !header.starts_with(' ')
            && let Some(id) = header.strip_suffix(':')
        {
            job = Some(id.to_owned());
        }
        if line == "    continue-on-error: true" {
            assert!(found.insert(job.clone().expect("job header")));
        }
    }
    found
}

#[test]
fn optional_seed_failure_isolated_for_root_and_nested_workspaces_only() {
    let workflow = workflow();
    let seeds = BTreeSet::from([
        "dependency-seed".to_owned(),
        "dependency-seed-nested".to_owned(),
    ]);
    let actual = workflow
        .ir
        .jobs
        .iter()
        .filter(|(_, job)| job.is_dependency_seed())
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, seeds);
    for id in &seeds {
        let seed = &workflow.ir.jobs[id];
        assert_eq!(seed.needs, ["plan"]);
        assert!(
            seed.steps
                .iter()
                .any(|step| step.role == Some(StepRole::VerifyRustManifest))
        );
        assert!(!workflow.ir.jobs["required"].needs.contains(id));
        assert!(
            !seed
                .steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::TaskExecution { .. })
                    || step.role == Some(StepRole::MatrixReportUpload))
        );
    }
    let yaml = render(&workflow).expect("render production seeds and strict real jobs");
    assert_eq!(
        optional_jobs(&yaml),
        seeds,
        "no real crate, validator, Plan or Required may be optional"
    );
}

#[test]
fn mixed_seed_quality_report_unknown_and_privileged_roles_fail_closed() {
    let workflow = workflow();
    let seed_id = "dependency-seed";
    let quality = workflow.ir.jobs["rust-demo"]
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .expect("real quality task")
        .clone();
    let report = workflow.ir.jobs["rust-demo"]
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::MatrixReportUpload))
        .expect("report")
        .clone();
    let plan = workflow.ir.jobs["plan"]
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::PlanProducer))
        .expect("Plan producer")
        .clone();
    for step in [quality, report, plan] {
        let mut mixed = workflow.clone();
        mixed
            .ir
            .jobs
            .get_mut(seed_id)
            .expect("seed")
            .steps
            .push(step);
        let error = render(&mixed).expect_err("mixed seed must fail");
        assert!(
            error.to_string().contains("mixed_or_invalid_seed"),
            "{error}"
        );
    }
    let mut untyped = workflow.clone();
    let step = untyped
        .ir
        .jobs
        .get_mut(seed_id)
        .expect("seed")
        .steps
        .iter_mut()
        .find(|step| step.role == Some(StepRole::MbxProducerMark))
        .expect("producer mark");
    step.role = None;
    assert!(render(&untyped).is_err());
}

#[test]
fn inherited_privileges_require_an_explicit_read_only_seed_override() {
    let workflow = workflow();
    let seed_id = "dependency-seed";
    for permissions in [
        Permissions {
            contents: PermissionLevel::Write,
            ..Permissions::default()
        },
        Permissions {
            id_token: PermissionLevel::Write,
            ..Permissions::default()
        },
    ] {
        let mut privileged = workflow.clone();
        privileged.ir.permissions = permissions;
        for (id, job) in &mut privileged.ir.jobs {
            if id != seed_id {
                job.permissions = Some(Permissions::default());
            }
        }
        assert!(privileged.ir.jobs[seed_id].permissions.is_none());
        let error = privileged
            .ir
            .validate()
            .expect_err("inherited seed write grant");
        assert!(
            error.to_string().contains("mixed_or_invalid_seed"),
            "{error}"
        );
        privileged
            .ir
            .jobs
            .get_mut(seed_id)
            .expect("seed")
            .permissions = Some(Permissions::default());
        assert!(privileged.ir.validate().is_ok());
    }
}

#[test]
fn optional_seed_role_never_follows_job_names_or_mixed_markers() {
    let workflow = workflow();
    let expected = BTreeSet::from([
        "dependency-seed".to_owned(),
        "dependency-seed-nested".to_owned(),
    ]);
    let mut named = workflow.clone();
    named
        .ir
        .jobs
        .get_mut("rust-demo")
        .expect("real job")
        .display_name = "Dependency seed".to_owned();
    assert!(!named.ir.jobs["rust-demo"].is_dependency_seed());
    assert_eq!(
        optional_jobs(&render(&named).expect("display name is not authority")),
        expected
    );
    let marker = workflow.ir.jobs["dependency-seed"]
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::MbxSeedBuild))
        .expect("typed producer")
        .clone();
    let mut mixed = workflow;
    mixed
        .ir
        .jobs
        .get_mut("rust-demo")
        .expect("real job")
        .steps
        .push(marker);
    let error = render(&mixed).expect_err("a seed marker cannot mask a real job");
    assert!(
        error.to_string().contains("mixed_or_invalid_seed"),
        "{error}"
    );
}

#[test]
fn rust_manifest_setup_role_rejects_non_shell_payload() {
    let mut workflow = workflow();
    let non_shell = workflow.ir.jobs["rust-demo"]
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .expect("real execution")
        .kind
        .clone();
    let verifier = workflow
        .ir
        .jobs
        .get_mut("dependency-seed")
        .expect("seed")
        .steps
        .iter_mut()
        .find(|step| step.role == Some(StepRole::VerifyRustManifest))
        .expect("typed Rust integrity setup");
    verifier.kind = non_shell;
    assert!(render(&workflow).is_err());
}
