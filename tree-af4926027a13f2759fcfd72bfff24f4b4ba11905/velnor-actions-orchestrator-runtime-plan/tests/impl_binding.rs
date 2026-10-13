//! Plan-identity binding gate: failure precedence and lane vocabulary.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck,
};
use velnor_actions_contract_planning::{
    CachePolicy, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
};
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, ToolCatalog};
use velnor_actions_orchestrator_runtime_plan::binding::{bind_check, parse_lane_variant};

const TASK: &str = "stack/mise/demo/check/default";
const JOB: &str = "check-demo";

/// Platform matching the test host.
fn host_platform() -> CheckPlatform {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => CheckPlatform::LinuxX64,
        ("macos", "aarch64") => CheckPlatform::MacosArm64,
        ("macos", "x86_64") => CheckPlatform::MacosX64,
        (os, arch) => panic!("unsupported test host {os}/{arch}"),
    }
}

fn proposal(task_id: &str) -> ProposedTask {
    ProposedTask {
        task_id: task_id.to_owned(),
        stack_id: "mise".to_owned(),
        component_id: "demo".to_owned(),
        task_kind: "check".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Lightweight,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "root".to_owned(),
            unit_path: "mise.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "mise".to_owned(),
            test_runner: "mise".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: Vec::new(),
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

fn discovered(task_id: &str, platform: CheckPlatform, inputs: &[&str]) -> DiscoveredCheck {
    DiscoveredCheck {
        check: MiseCheck {
            id: "demo".to_owned(),
            task: "check".to_owned(),
            directory: ".".to_owned(),
            runner: CheckRunner {
                label: "test".to_owned(),
                platform,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            inputs: Vec::new(),
            tools: Vec::new(),
            system_tools: Vec::new(),
            evidence: None,
            timeout_minutes: 30,
        },
        proposal: proposal(task_id),
        config_inputs: inputs.iter().map(ToString::to_string).collect(),
        task_config: String::new(),
        tool_specs: Vec::new(),
        qualified_tools: Vec::new(),
        qualification_digest: String::new(),
        config_source: String::new(),
    }
}

fn entry() -> MatrixEntry {
    let digest = format!("b3-{}", "01".repeat(32));
    MatrixEntry::derive(
        "mise",
        TASK,
        "true",
        &digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("check".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()))]),
        },
        &format!("b3-{}", "02".repeat(32)),
        "local",
        JOB,
    )
    .expect("entry derives")
}

/// Bare plan carrying the run key, generator, and obligations the gate reads.
fn plan() -> Plan {
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "test-plan".to_owned(),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "test".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "test".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: format!("b3-{}", "01".repeat(32)),
            input_digest: format!("b3-{}", "02".repeat(32)),
            closure_digest: format!("b3-{}", "03".repeat(32)),
            baseline_proof: None,
        }],
        matrix: PlanMatrix { include: vec![] },
        task_ids: vec![TASK.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    }
}

fn deadline() -> CheckDeadline {
    CheckDeadline::from_start(Instant::now(), Duration::from_secs(60)).expect("deadline")
}

#[test]
fn single_lane_parses_to_no_variant() {
    assert!(
        parse_lane_variant("single")
            .expect("single parses")
            .is_none()
    );
}

#[test]
fn hosted_lane_parses_to_hosted() {
    assert!(matches!(
        parse_lane_variant("hosted").expect("hosted parses"),
        Some(variant) if matches!(variant, velnor_actions_contract_workflow::NamedCheckLaneVariant::Hosted)
    ));
}

#[test]
fn scale_set_lane_parses_to_scale_set() {
    assert!(matches!(
        parse_lane_variant("scale_set").expect("scale_set parses"),
        Some(variant) if matches!(variant, velnor_actions_contract_workflow::NamedCheckLaneVariant::ScaleSet)
    ));
}

#[test]
fn unknown_lane_word_refuses() {
    for word in ["", "multi", "SINGLE", "scale-set"] {
        let err = parse_lane_variant(word).expect_err("unknown lane refuses");
        assert!(
            err.to_string().contains("check_lane_variant_invalid"),
            "{err}"
        );
    }
}

#[test]
fn task_mismatch_refuses_identity() {
    let temp = TempDir::new().expect("temp");
    let item = discovered("stack/mise/other/check/default", host_platform(), &[]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("task mismatch refuses");
    assert!(err.to_string().contains("check_task_identity"), "{err}");
}

#[test]
fn empty_caller_task_refuses_identity() {
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, host_platform(), &[]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        "",
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("empty task refuses");
    assert!(err.to_string().contains("check_task_identity"), "{err}");
}

#[test]
fn mismatched_os_refuses_platform() {
    let platform = if std::env::consts::OS == "linux" {
        CheckPlatform::MacosArm64
    } else {
        CheckPlatform::LinuxX64
    };
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, platform, &[]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("foreign os refuses");
    assert!(err.to_string().contains("check_host_platform"), "{err}");
}

#[test]
fn mismatched_arch_refuses_platform() {
    let platform = if std::env::consts::ARCH == "aarch64" {
        CheckPlatform::MacosX64
    } else {
        CheckPlatform::MacosArm64
    };
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, platform, &[]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("foreign arch refuses");
    assert!(err.to_string().contains("check_host_platform"), "{err}");
}

#[test]
fn escaping_input_refuses_link() {
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, host_platform(), &["../escape"]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("escape refuses");
    assert!(err.to_string().contains("check_path_escape"), "{err}");
}

#[test]
fn absolute_input_refuses_link() {
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, host_platform(), &["/absolute"]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("absolute refuses");
    assert!(err.to_string().contains("check_path_escape"), "{err}");
}

#[test]
fn bound_identity_reaches_lane_derivation() {
    let temp = TempDir::new().expect("temp");
    let item = discovered(TASK, host_platform(), &[]);
    let plan = plan();
    let entry = entry();
    let err = bind_check(
        temp.path(),
        &item,
        &plan,
        &entry,
        TASK,
        &ToolCatalog::pinned(),
        deadline(),
    )
    .expect_err("bare temp root cannot derive lanes");
    let message = err.to_string();
    for early in ["check_task_identity", "check_host_platform", "check_path_"] {
        assert!(!message.contains(early), "stopped early: {message}");
    }
}
