//! Structural integration coverage for the isolated Rust source producer.

use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::{
    CacheMode, Job, JobTimeout, ProposedTask, Step, StepKind, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
    TargetRecord, TaskGroup, TaskKind, TestRunner, WorkspaceRecord,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::clippy_groups::ClippyMemoryPlan;
use crate::discover::{Discovery, PlannedWorkspace};

const RUNNER: &str = "ubuntu-26.04";
const PACKAGE_ID: &str = "demo 0.1.0";
const TASK_ID: &str = "stack/rust/root/test/default";
const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Fixture {
    repository: TempDir,
    discovery: Discovery,
    jobs: BTreeMap<String, Job>,
    roots: Vec<String>,
    task_id: String,
}

#[test]
fn rust_source_cohort_merges_selected_task_and_plan_fallback_without_cycle() {
    let fixture = fixture();
    let mut jobs = fixture.jobs;
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let helpers = crate::source_prep::producer::insert_producers(
        &mut jobs,
        fixture.repository.path(),
        &fixture.discovery,
        &ToolCatalog::pinned(),
        &setup,
        &fixture.roots,
        VERSION,
    )
    .expect("Rust source producers");

    let (producer_id, producer) = cargo_producer(&jobs);
    let metadata = producer.source_producer.as_ref().expect("Cargo metadata");
    assert_eq!(metadata.selection.tasks, [fixture.task_id.clone()]);
    assert!(metadata.selection.cargo_fallback);
    assert!(!metadata.selection.unconditional);
    assert_eq!(producer.needs, vec![PLAN_JOB_ID.to_owned()]);
    assert!(!producer.needs.iter().any(|need| need == producer_id));

    let plan = jobs.get(PLAN_JOB_ID).expect("Plan job");
    assert!(!plan.needs.iter().any(|need| need == producer_id));
    assert!(plan.steps.iter().all(|step| !is_legacy_source_writer(step)));

    let consumer = jobs.get("rust-demo").expect("Rust consumer");
    assert_eq!(
        consumer.needs,
        vec![PLAN_JOB_ID.to_owned(), producer_id.to_owned()]
    );
    assert!(
        consumer
            .steps
            .iter()
            .all(|step| !is_legacy_source_writer(step))
    );
    assert!(!helpers.is_empty());
}

#[test]
fn generated_consumers_have_no_legacy_source_writers_and_bind_returned_helper_exactly() {
    let fixture = fixture();
    let mut jobs = fixture.jobs;
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let helpers = crate::source_prep::producer::insert_producers(
        &mut jobs,
        fixture.repository.path(),
        &fixture.discovery,
        &ToolCatalog::pinned(),
        &setup,
        &fixture.roots,
        VERSION,
    )
    .expect("Rust source producers");

    assert!(
        [PLAN_JOB_ID, "rust-demo"]
            .into_iter()
            .map(|id| jobs.get(id).expect("generated consumer"))
            .flat_map(|job| &job.steps)
            .all(|step| !is_legacy_source_writer(step))
    );

    let (_, producer) = cargo_producer(&jobs);
    let helper = helpers
        .iter()
        .find(|record| {
            record.invocation().descriptor().operation()
                == velnor_actions_contract::SourceBoundOperation::RustSourceProducer
        })
        .expect("returned Rust source helper");
    let verify = producer
        .steps
        .iter()
        .find(|step| step.name == "Verify public Cargo sources")
        .expect("source verification step");
    let StepKind::SourceBoundHelper { invocation, env } = &verify.kind else {
        panic!("source verification must use an owner-qualified helper");
    };
    assert_eq!(invocation, helper.invocation());
    assert_eq!(env, helper.environment());
    let identity = producer
        .source_producer
        .as_ref()
        .expect("Cargo metadata")
        .source_identity
        .as_str();
    assert_eq!(helper.environment()["VELNOR_SOURCE_IDENTITY"], identity);
}

fn cargo_producer(jobs: &BTreeMap<String, Job>) -> (&str, &Job) {
    jobs.iter()
        .find(|(_, job)| {
            job.source_producer.as_ref().is_some_and(|producer| {
                producer.role == velnor_actions_contract::SourceProducerRole::Cargo
            })
        })
        .map(|(id, job)| (id.as_str(), job))
        .expect("Cargo source producer")
}

fn is_legacy_source_writer(step: &Step) -> bool {
    match &step.kind {
        StepKind::Action { .. } => {
            step.name == velnor_actions_workflow_renderer::cache_p08::SAVE_SOURCES_NAME
        }
        StepKind::Shell { env, .. } => env
            .get("VELNOR_SNAPSHOT_LAYER")
            .is_some_and(|layer| layer == "sources"),
        StepKind::SourceBoundHelper { .. } | StepKind::Internal { .. } => false,
    }
}

fn fixture() -> Fixture {
    let repository = tempfile::tempdir().expect("repository");
    let root = repository.path();
    fs::create_dir_all(root.join("src")).expect("source directory");
    fs::create_dir_all(root.join("other")).expect("secondary manifest directory");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1.0\"\n",
    )
    .expect("manifest");
    fs::write(
        root.join("other/Cargo.toml"),
        "[package]\nname = \"other\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("secondary manifest");
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").expect("source");
    fs::write(root.join("Cargo.lock"), lockfile()).expect("lockfile");

    let task = rust_task();
    let task_id = task.task_id.clone();
    let discovery = discovery(task);
    let roots = vec![String::new()];
    let jobs = jobs_with_generated_consumers(&discovery, &roots);
    Fixture {
        repository,
        discovery,
        jobs,
        roots,
        task_id,
    }
}

fn lockfile() -> &'static str {
    "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\ndependencies = [\"serde 1.0.0\"]\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n"
}

fn rust_task() -> ProposedTask {
    velnor_actions_rust::propose_task(&TaskGroup {
        task_id: TASK_ID.to_owned(),
        package_id: PACKAGE_ID.to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Test,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
    .expect("fixture task")
}

fn discovery(task: ProposedTask) -> Discovery {
    Discovery {
        rust_inventory: None,
        raw_inventories: Vec::new(),
        statuses: Vec::new(),
        workspaces: vec![PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: String::new(),
                members: vec![PACKAGE_ID.to_owned()],
                packages: vec![PackageRecord {
                    id: PACKAGE_ID.to_owned(),
                    name: "demo".to_owned(),
                    version: "0.1.0".to_owned(),
                    // Deliberately cannot reconstruct selected payload metadata;
                    // Task and Plan must therefore share the complete descriptor.
                    manifest: "other/Cargo.toml".to_owned(),
                    external: false,
                    in_workspace: true,
                    targets: vec![TargetRecord {
                        kind: "lib".to_owned(),
                        name: "demo".to_owned(),
                        test: true,
                        doctest: true,
                        required_features: Vec::new(),
                    }],
                    features: Vec::new(),
                    has_build_script: false,
                }],
                edges: Vec::new(),
                skipped_edges: Vec::new(),
            },
            profile: RustExecutionProfile {
                compile_driver: CompileDriver::Cargo,
                test_runner: TestRunner::CargoTest,
                evidence: Vec::new(),
                driver_source: ProfileSource::Detected,
                runner_source: ProfileSource::Detected,
                nextest_profile: NextestProfile::Default,
                nextest_config: None,
            },
            recommendations: Vec::new(),
            findings: Vec::new(),
        }],
        proposals: vec![task],
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

fn jobs_with_generated_consumers(discovery: &Discovery, roots: &[String]) -> BTreeMap<String, Job> {
    let built = crate::crate_jobs::build_crate_jobs(
        RUNNER,
        WorkflowPolicy::ConsumerV1,
        discovery,
        &ToolCatalog::pinned(),
        roots,
        &[],
        None,
        1,
    )
    .expect("generated Rust consumers");
    let mut jobs = BTreeMap::from([(PLAN_JOB_ID.to_owned(), job("Plan", true))]);
    jobs.extend(built.jobs);
    jobs
}

fn job(display_name: &str, plan: bool) -> Job {
    Job {
        cache_mode: Some(CacheMode::Read),
        display_name: display_name.to_owned(),
        runs_on: RUNNER.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: if plan {
            Vec::new()
        } else {
            vec![PLAN_JOB_ID.to_owned()]
        },
        condition: Some("success()".to_owned()),
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: vec![Step {
            id: None,
            name: if plan {
                "Early plan".to_owned()
            } else {
                "Fetch Cargo sources".to_owned()
            },
            condition: None,
            kind: if plan {
                StepKind::Internal {
                    operation: velnor_actions_workflow_renderer::steps::EARLY_PLAN_OPERATION
                        .to_owned(),
                }
            } else {
                StepKind::Internal {
                    operation: "fetch-fixture".to_owned(),
                }
            },
        }],
    }
}
