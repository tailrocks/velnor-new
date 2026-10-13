//! Remediation cases: WF/TASK audit rows (runner, matrix, artifacts).
use crate::impl_contract_ids::{GROUP, sample_entry};
use crate::impl_shared_fixtures::{sample_plan, valid_config};
use std::collections::BTreeMap;
use velnor_actions_contract::{ContractError, digest_b3, run_key_for_ci};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_release::require_release_version;
use velnor_actions_contract_workflow::{
    MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, PlanRunner, check_matrix_agreement,
    matrix_json_bytes, plan_json_bytes,
};

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
    let root = include_str!("../src/lib.rs");
    assert!(
        root.contains("Must not own")
            && root.contains("configuration")
            && root.contains("release manifests")
            && root.contains("detection")
            && root.contains("workflow IR"),
        "crate ownership forbids must stay documented"
    );
    assert_contract_modules(root);
}

/// Exactly the contract's public modules, sorted: additions deliberate.
fn assert_contract_modules(root: &str) {
    let mut modules: Vec<&str> = root
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .filter_map(|line| line.strip_suffix(';'))
        .collect();
    modules.sort_unstable();
    assert_eq!(
        modules,
        [
            "archive",
            "cachekey",
            "canonical",
            "closure",
            "errors",
            "extension_schemas",
            "ids",
            "marker",
            "secrets",
            "stack",
            "strict_json",
            "vcs",
        ]
    );
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
        assert_eq!(runner.validate(), Ok(()));
        let text = serde_json::to_string(&runner).expect("serialize");
        let back: PlanRunner = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.label, "ubuntu-24.04");
        assert_eq!(back.selection, selection);
    }
    let alias = PlanRunner {
        label: "ubuntu-latest".to_owned(),
        selection: RunnerSelection::ConfigOverride,
    };
    let err = alias.validate().expect_err("alias label");
    assert!(err.to_string().contains("unsupported_label"));
    let run_key = run_key_for_ci(4, 1);
    let mut plan = sample_plan(&run_key)?;
    plan.runner.selection = RunnerSelection::ConfigOverride;
    plan.runner.label = "ubuntu-24.04".to_owned();
    plan.validate()?;
    plan.runner.label = "ubuntu-latest".to_owned();
    assert!(plan.validate().is_err());
    Ok(())
}

#[test]
fn wf_matrix_entry_requires_registered_stack() -> Result<(), ContractError> {
    use velnor_actions_contract_workflow::{ExecuteTaskIds, MatrixEntry};
    let run_key = run_key_for_ci(6, 1);
    sample_entry(&run_key)?.validate(&run_key)?;
    let bogus = MatrixEntry::derive(
        "bogus",
        GROUP,
        "mise exec --no-config rust@1.98.1 -- cargo clippy --locked",
        &digest_b3(b"task-bytes"),
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::new(),
        },
        &digest_b3(b"entry-inputs"),
        &run_key,
        "plan",
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
fn wf_plan_artifact_bytes_and_agreement() -> Result<(), ContractError> {
    assert_eq!(PLAN_JSON_FILENAME, "plan.json");
    assert_eq!(MATRIX_JSON_FILENAME, "matrix.json");
    let run_key = run_key_for_ci(10, 1);
    let plan = sample_plan(&run_key)?;
    let plan_bytes = plan_json_bytes(&plan)?;
    let matrix_bytes = matrix_json_bytes(&plan.matrix)?;
    let matrix_text = String::from_utf8(matrix_bytes.clone()).expect("utf8");
    assert!(matrix_text.starts_with("{\"include\":["));
    assert_eq!(check_matrix_agreement(&plan.matrix, &matrix_bytes), Ok(()));
    let pretty = serde_json::to_string_pretty(&plan.matrix).expect("pretty");
    assert_eq!(
        check_matrix_agreement(&plan.matrix, pretty.as_bytes()),
        Ok(())
    );
    let mut tampered: serde_json::Value = serde_json::from_slice(&matrix_bytes).expect("json");
    tampered["include"][0]["input_digest"] = serde_json::json!("b3-tampered");
    let tampered_bytes = serde_json::to_vec(&tampered).expect("bytes");
    let err = check_matrix_agreement(&plan.matrix, &tampered_bytes).expect_err("tamper");
    assert_eq!(
        err,
        ContractError::identity("matrix.json", "matrix_agreement_mismatch")
    );
    assert!(check_matrix_agreement(&plan.matrix, b"not json").is_err());
    let plan_text = String::from_utf8(plan_bytes).expect("utf8");
    assert!(plan_text.contains("\"matrix\":{\"include\":["));
    Ok(())
}

#[test]
fn gap_release_gate_rejects_non_release() {
    assert_eq!(require_release_version("0.1.0", "release"), Ok(()));
    assert_eq!(require_release_version("2026.9.16", "release"), Ok(()));
    for version in ["0.1.0-rc.1", "1.2", "1.2.3.4", "", "latest", "v1.2.3"] {
        let err = require_release_version(version, "release").expect_err("non-release");
        assert_eq!(
            err.to_string(),
            "release: version: non_release_build",
            "{version}"
        );
    }
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
