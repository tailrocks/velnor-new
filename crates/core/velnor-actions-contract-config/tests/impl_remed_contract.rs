//! Remediation cases: ARCH/GEN/WF/TASK audit rows.
use crate::impl_shared_fixtures::{GROUP, MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{ContractError, digest_b3, plan_id_for_run, run_key_for_ci};
use velnor_actions_contract_config::config::ActionsConfig;
use velnor_actions_contract_config::{
    DiscoveryConfig, ResourcesConfig, RunnerSelection, RustStackConfig, StacksConfig,
    TestShardingConfig, VelnorConfig, WorkflowConfig, WorkflowPolicy,
};
use velnor_actions_contract_release::{
    AGENTS_MD_PATH, CLAUDE_MD_PATH, DECLARED_GITHUB_FORMATS, find_github_format,
    is_declared_github_format,
};
use velnor_actions_contract_workflow::{
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, Trust, WorkflowEvent,
};

/// Valid config shared by remediation cases.
pub(crate) fn valid_config() -> VelnorConfig {
    VelnorConfig {
        checks: Vec::new(),
        qualified_tools: Vec::new(),
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: velnor_actions_contract_config::GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
            tasks: Vec::new(),
        },
        resources: ResourcesConfig {
            compiler_process_budget: 2,
            test_process_budget: 2,
        },
        test_sharding: TestShardingConfig {
            default_shards: 1,
            by_manifest: BTreeMap::new(),
        },
        stacks: StacksConfig {
            ignore: vec![],
            rust: None,
            tofu: None,
        },
        discovery: DiscoveryConfig { exclude: vec![] },
        actions: ActionsConfig::default(),
        execution: None,
    }
}

/// Valid plan shared by remediation cases.
pub(crate) fn sample_plan(run_key: &str) -> Result<Plan, ContractError> {
    let entry = sample_entry(run_key)?;
    Ok(Plan {
        schema: 1,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key)?,
        base: None,
        head: "ab".repeat(20),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    })
}

#[test]
fn arch_default_rust_config_is_single_default_host() {
    let default = RustStackConfig::default_config();
    assert_eq!(default.configurations.len(), 1);
    let only = &default.configurations[0];
    assert_eq!(only.name, "default");
    assert_eq!(only.target, "host");
    assert_eq!(default_config_summary(), "default/host");
    assert!(default.validate("cfg").is_ok());
}

/// Render the documented default as `name/target` for the fallback pin.
fn default_config_summary() -> String {
    let default = RustStackConfig::default_config();
    default
        .configurations
        .iter()
        .map(|config| format!("{}/{}", config.name, config.target))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn arch_config_rejects_shell_yaml_uses_task_keys() {
    let base = serde_json::to_value(valid_config()).expect("serialize");
    for (section, key) in [
        ("workflow", "shell"),
        ("workflow", "run"),
        ("stacks", "yaml"),
        ("stacks", "uses"),
        ("stacks", "tasks"),
        ("resources", "shell"),
        ("discovery", "uses"),
    ] {
        let mut value = base.clone();
        value[section][key] = serde_json::json!("evil");
        let err = serde_json::from_value::<VelnorConfig>(value).expect_err("must reject");
        assert!(err.to_string().contains("unknown field"), "{section}.{key}");
    }
    let mut top = base.clone();
    top["shell"] = serde_json::json!(["rm -rf /"]);
    assert!(serde_json::from_value::<VelnorConfig>(top).is_err());
}

#[test]
fn arch_unknown_keys_fail_with_unknown_config_field() {
    let err = ContractError::unknown_config_field("cfg.toml", "stacks.rust.shell");
    assert_eq!(
        err.to_string(),
        "cfg.toml: stacks.rust.shell: unknown_config_field"
    );
    let json = "{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"r\",\"commit\":\"ab00000000000000000000000000000000000000\",\"shell\":\"x\",\"targets\":[]}";
    let Err(ContractError::Config {
        problem, key_path, ..
    }) = velnor_actions_contract_release::ReleaseManifest::parse_json(json, "m.json")
    else {
        panic!("unknown key must fail");
    };
    assert_eq!(problem, "unknown_config_field");
    assert_eq!(key_path, "shell");
    let serde_err = "unknown field `shell`, expected `schema`";
    let mapped = ContractError::map_decode_error("cfg.toml", serde_err);
    assert_eq!(mapped.to_string(), "cfg.toml: shell: unknown_config_field");
    let toml_err = "TOML parse error at line 3, column 1\n  |\n3 | shell = \"x\"\n  | ^^^^^\nunknown field 'shell', expected 'schema'";
    let mapped = ContractError::map_decode_error("cfg.toml", toml_err);
    assert_eq!(mapped.to_string(), "cfg.toml: shell: unknown_config_field");
    let other = ContractError::map_decode_error("cfg.toml", "expected value, found eof");
    assert!(other.to_string().starts_with("cfg.toml: document:"));
}

#[test]
fn arch_unknown_schema_fails_with_unsupported_schema() {
    let mut config = valid_config();
    config.schema = 99;
    let err = config.validate("cfg").expect_err("schema 99 must fail");
    assert!(matches!(err, ContractError::UnsupportedSchema { .. }));
    assert!(err.to_string().starts_with("unsupported_schema:"));
}

#[test]
fn arch_required_schema_has_no_default() {
    let mut value = serde_json::to_value(valid_config()).expect("serialize");
    value.as_object_mut().expect("object").remove("schema");
    let err = serde_json::from_value::<VelnorConfig>(value).expect_err("missing schema");
    assert!(err.to_string().contains("missing field `schema`"));
}

#[test]
fn gen_declared_github_formats_gate_new_writers() {
    assert_eq!(DECLARED_GITHUB_FORMATS.len(), 4);
    let agents = find_github_format(AGENTS_MD_PATH).expect("agents");
    assert_eq!(agents.owner, "velnor-actions-workflow-renderer");
    let claude = find_github_format(CLAUDE_MD_PATH).expect("claude");
    assert_eq!(claude.owner, "velnor-actions-workflow-renderer");
    let actionlint = find_github_format(".github/actionlint.yaml").expect("actionlint");
    assert_eq!(actionlint.owner, "velnor-actions-actionlint");
    let workflow = find_github_format(".github/workflows/ci.yml").expect("workflow");
    assert_eq!(workflow.owner, "velnor-actions-workflow-renderer");
    assert!(find_github_format(".github/workflows/other.yml").is_none());
    assert!(is_declared_github_format(AGENTS_MD_PATH));
    assert!(is_declared_github_format(CLAUDE_MD_PATH));
    assert!(is_declared_github_format(".github/actionlint.yaml"));
    assert!(!is_declared_github_format("mise.toml"));
}

#[test]
fn gen_task_ids_preserved_through_plan_and_matrix() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(9, 1);
    let plan = sample_plan(&run_key)?;
    plan.validate()?;
    let text = serde_json::to_string(&plan).expect("serialize");
    let round_trip: Plan = serde_json::from_str(&text).expect("deserialize");
    round_trip.validate()?;
    assert_eq!(round_trip.task_ids, vec![TASK.to_owned()]);
    assert_eq!(round_trip.obligations[0].task_id, TASK);
    assert!(round_trip.matrix.include[0].id.ends_with(GROUP));
    assert!(text.contains(TASK));
    Ok(())
}
