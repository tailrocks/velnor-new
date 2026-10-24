use super::{check, fixture, hosted, named_workflow};
use crate::workflow::check_jobs::build_check_jobs;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::{
    DetectedProject, DetectionStatus, JobConclusion, RequiredJobResult, StepKind, WorkflowPolicy,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn fork_admission_skips_external_runner_and_required_rejects_skip() {
    let runner = CheckRunner {
        label: "native-check-scale-set".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    };
    let (_root, discovery) = fixture(&[check("native", runner)]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("external check");
    assert_eq!(
        jobs[0].1.condition.as_deref(),
        Some(EPHEMERAL_CHECK_ADMISSION_CONDITION)
    );
    assert!(
        EPHEMERAL_CHECK_ADMISSION_CONDITION.contains("head.repo.full_name == github.repository")
    );
    let mut signals = crate::cover::Signals::default();
    crate::required_dispositions::fold_planned_jobs(
        &[],
        &[RequiredJobResult {
            job_id: "check-native".to_owned(),
            conclusion: JobConclusion::Skipped,
        }],
        &mut signals,
        &mut std::collections::BTreeSet::new(),
    );
    assert!(signals.not_run);
}

#[test]
fn required_rejects_failed_or_missing_named_check() {
    for conclusion in [JobConclusion::Missing, JobConclusion::Failure] {
        let mut signals = crate::cover::Signals::default();
        crate::required_dispositions::fold_planned_jobs(
            &[],
            &[RequiredJobResult {
                job_id: "check-ffi".to_owned(),
                conclusion,
            }],
            &mut signals,
            &mut std::collections::BTreeSet::new(),
        );
        assert!(signals.failed, "{conclusion:?}");
    }
}

#[test]
fn native_check_without_catalog_tools_adds_no_rust_install() {
    let mut native = check("native", hosted("macos-15", CheckPlatform::MacosArm64));
    native.tools.clear();
    let (_root, discovery) = fixture(&[native]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("native system check");
    let names: Vec<&str> = jobs[0]
        .1
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    assert!(!names.contains(&"Prepare pinned tools"));
    assert!(!names.contains(&"Prepare Rust components"));
    assert!(names.contains(&"Execute named check"));
}

#[test]
fn ignored_rust_candidate_keeps_plan_inventory_toolchain() {
    let (root, mut discovery) = fixture(&[]);
    discovery.statuses.push(DetectionStatus::Ignored {
        project: DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
        reason: "configured_ignore".to_owned(),
    });
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("defaulted config");
    let workflow = named_workflow(root.path(), &config, &discovery);
    let plan = &workflow.ir.jobs["plan"];
    let pinned_tools = plan
        .steps
        .iter()
        .find(|step| step.name == "Prepare pinned tools")
        .expect("plan inventory prepares its exact Rust compiler");
    assert!(matches!(
        &pinned_tools.kind,
        StepKind::Shell { run, .. }
            if run.contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust))
    ));
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components"),
        "an ignored candidate has no selected plan-owned Format step"
    );
}
