//! Remediation cases: ARCH/GEN/WF/TASK audit rows.
use crate::impl_contract_ids::{GROUP, MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::config::ActionsConfig;
use velnor_actions_contract::{
    BaselineStatus, ContractError, DECLARED_GITHUB_FORMATS, DiscoveryConfig, ObligationDecision,
    Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner,
    ResourcesConfig, RunnerSelection, RustStackConfig, StacksConfig, TestShardingConfig, Trust,
    VelnorConfig, WorkflowConfig, WorkflowEvent, WorkflowPolicy, digest_b3, find_github_format,
    is_declared_github_format, plan_id_for_run, run_key_for_ci,
};

/// Valid config shared by remediation cases.
pub(crate) fn valid_config() -> VelnorConfig {
    VelnorConfig {
        schema: 1,
        workflow: WorkflowConfig {
            name: "CI".to_owned(),
            policy: WorkflowPolicy::ConsumerV1,
            default_branch: None,
            generator_validation: velnor_actions_contract::GeneratorValidation::Bootstrap,
            max_parallel_jobs: 2,
            runner_label: None,
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
        },
        discovery: DiscoveryConfig { exclude: vec![] },
        actions: ActionsConfig::default(),
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
        baseline: PlanBaseline {
            status: BaselineStatus::Unavailable,
            base_commit: None,
            run_id: None,
            artifact_id: None,
            artifact_name: None,
            manifest_digest: None,
            reason: Some("no_entry".to_owned()),
        },
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
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
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
    let json =
        "{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"r\",\"shell\":\"x\",\"targets\":[]}";
    let Err(ContractError::Config {
        problem, key_path, ..
    }) = velnor_actions_contract::ReleaseManifest::parse_json(json, "m.json")
    else {
        panic!("unknown key must fail");
    };
    assert_eq!(problem, "unknown_config_field");
    assert_eq!(key_path, "shell");
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
    assert_eq!(DECLARED_GITHUB_FORMATS.len(), 2);
    let actionlint = find_github_format(".github/actionlint.yaml").expect("actionlint");
    assert_eq!(actionlint.owner, "velnor-actions-actionlint");
    let workflow = find_github_format(".github/workflows/velnor.yml").expect("workflow");
    assert_eq!(workflow.owner, "velnor-actions-workflow-renderer");
    assert!(find_github_format(".github/workflows/other.yml").is_none());
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

#[test]
fn wf_contract_surface_has_no_utility_fields() {
    let value = serde_json::to_value(valid_config()).expect("serialize");
    let top: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        top,
        [
            "actions",
            "discovery",
            "resources",
            "schema",
            "stacks",
            "test_sharding",
            "workflow"
        ]
    );
    let section = |name: &str| -> Vec<String> {
        let mut keys: Vec<String> = value[name]
            .as_object()
            .expect("section")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        section("workflow").join(","),
        "generator_validation,max_parallel_jobs,name,policy"
    );
    assert_eq!(section("stacks").join(","), "ignore");
    let text = serde_json::to_string(&value).expect("text");
    for forbidden in ["shell", "uses", "yaml", "command", "argv", "mise_task"] {
        assert!(!text.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn wf_plan_runner_records_label_and_provenance() -> Result<(), ContractError> {
    for selection in [
        RunnerSelection::LatestDefault,
        RunnerSelection::ConfigOverride,
    ] {
        let runner = PlanRunner {
            label: "ubuntu-24.04".to_owned(),
            selection,
        };
        let text = serde_json::to_string(&runner).expect("serialize");
        let back: PlanRunner = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.label, "ubuntu-24.04");
        assert_eq!(back.selection, selection);
    }
    let run_key = run_key_for_ci(4, 1);
    let mut plan = sample_plan(&run_key)?;
    plan.runner.selection = RunnerSelection::ConfigOverride;
    plan.runner.label = "ubuntu-24.04".to_owned();
    plan.validate()?;
    Ok(())
}

#[test]
fn wf_matrix_entry_requires_registered_stack() -> Result<(), ContractError> {
    use velnor_actions_contract::{ExecuteTaskIds, MatrixEntry};
    let run_key = run_key_for_ci(6, 1);
    sample_entry(&run_key)?.validate(&run_key)?;
    let bogus = MatrixEntry::derive(
        "bogus",
        GROUP,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::new(),
        },
        &digest_b3(b"entry-inputs"),
        &run_key,
    )?;
    let err = bogus.validate(&run_key).expect_err("bogus stack");
    assert_eq!(
        err,
        ContractError::identity("stack_id", "unregistered_stack")
    );
    Ok(())
}

#[test]
fn wf_duplicate_matrix_id_or_key_is_collision() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(7, 1);
    let mut plan = sample_plan(&run_key)?;
    plan.validate()?;
    let entry = sample_entry(&run_key)?;
    plan.matrix.include.push(entry);
    let err = plan.validate().expect_err("duplicate id");
    assert!(matches!(err, ContractError::Collision(_)), "got {err}");
    Ok(())
}

#[test]
fn wf_plan_matrix_canonical_bytes_agree() -> Result<(), ContractError> {
    use velnor_actions_contract::canonical_json_bytes;
    let run_key = run_key_for_ci(8, 1);
    let plan = sample_plan(&run_key)?;
    plan.validate()?;
    let plan_bytes = canonical_json_bytes(&plan)?;
    let matrix_bytes = canonical_json_bytes(&plan.matrix)?;
    assert_eq!(plan_bytes, canonical_json_bytes(&plan)?);
    assert_eq!(matrix_bytes, canonical_json_bytes(&plan.matrix)?);
    let text = String::from_utf8(plan_bytes).expect("utf8");
    assert!(text.contains("\"matrix\":{\"include\":["));
    Ok(())
}

#[test]
fn task_driver_runner_switch_invalidates_evidence() -> Result<(), ContractError> {
    use crate::impl_contract_ids::sample_identity;
    use velnor_actions_contract::input_digest;
    let base = input_digest(&sample_identity())?;
    let mut switched = sample_identity();
    switched.configuration.compile_driver = "mbx".to_owned();
    assert_ne!(input_digest(&switched)?, base);
    let mut runner = sample_identity();
    runner.configuration.test_runner = "cargo_nextest".to_owned();
    assert_ne!(input_digest(&runner)?, base);
    assert_eq!(input_digest(&sample_identity())?, base);
    Ok(())
}
