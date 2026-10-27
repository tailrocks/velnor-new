// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

// gc1-t53 covers the group-ledger producer contract: zero numerics validate,
// and every digest constructor is deterministic and matches its documented
// construction through an independent canonical-JSON recomputation.
use super::*;
use super::{EffectiveInputDigestInputs, GroupDigestInputs, RecipeDigestInputs};

fn zero_numeric_job() -> JobBindingV1 {
    JobBindingV1 {
        schema: 1,
        repository_id: 0,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 4242,
        run_attempt: 3,
        job_id: 0,
        job_key: "group-write".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: "ab".repeat(20),
        source_sha: "cd".repeat(20),
        plan_digest: digest_b3_typed(b"gc1-t53-plan"),
    }
}

#[test]
fn zero_numerics_validate() -> Result<(), ContractError> {
    zero_numeric_job().validate()?;
    Ok(())
}

#[test]
fn run_identity_still_positive() {
    let mut job = zero_numeric_job();
    job.run_id = 0;
    assert!(job.validate().is_err(), "run_id zero still rejected");
    let mut job = zero_numeric_job();
    job.run_attempt = 0;
    assert!(job.validate().is_err(), "run_attempt zero still rejected");
}

fn manifest_entry() -> TaskExecutionManifestEntryV1 {
    TaskExecutionManifestEntryV1 {
        task_id: "task-1".to_owned(),
        execution_digest: "velnor-gc1/execution/v1".to_owned(),
        task_digest: digest_b3_typed(b"gc1-t53-task").as_str().to_owned(),
        toolchain_inputs: ToolchainInputs {
            tools: vec!["rust@1.89.0".to_owned()],
            components: vec!["clippy".to_owned()],
            compile_driver: "cargo".to_owned(),
            test_runner: "nextest".to_owned(),
        },
        argv: vec!["run".to_owned(), "--locked".to_owned()],
        env: BTreeMap::from([("KEY".to_owned(), "value".to_owned())]),
        matrix_id: "matrix-1".to_owned(),
        matrix_key: "key-1".to_owned(),
        report_helper_version: "0.1.57".to_owned(),
        matrix_max_parallel: Some(4),
    }
}

#[test]
fn plan_digest_covers_raw_bytes() {
    let bytes = br#"{"schema":1,"plan":"gc1-t53"}"#;
    assert_eq!(
        UnitLedgerV1::plan_digest_from_canonical_bytes(bytes).as_str(),
        digest_b3_typed(bytes).as_str(),
        "plan digest is b3 of the canonical plan bytes",
    );
}

#[test]
fn report_digest_vectors() {
    let bytes = br#"{"schema":1,"report":"gc1-t53"}"#;
    assert_eq!(
        UnitResultV1::report_digest_from_canonical_bytes(bytes).as_str(),
        digest_b3_typed(bytes).as_str(),
    );
    assert_eq!(
        UnitResultV1::missing_report_digest().as_str(),
        digest_b3_typed(UnitResultV1::NO_TASK_REPORT_DIGEST_INPUT.as_bytes()).as_str(),
        "missing report covers the specified absence encoding",
    );
    assert_ne!(
        UnitResultV1::missing_report_digest().as_str(),
        UnitResultV1::report_digest_from_canonical_bytes(b"").as_str(),
        "absence is never digest(empty)",
    );
}

#[test]
fn recipe_argv_input_map_manifest_fields() -> Result<(), ContractError> {
    let entry = manifest_entry();
    let recipe = UnitResultV1::recipe_digest_from_manifest_entry(&entry)?;
    let rebuilt = RecipeDigestInputs {
        task_digest: entry.task_digest.as_str(),
        toolchain_inputs: &entry.toolchain_inputs,
        report_helper_version: entry.report_helper_version.as_str(),
    };
    assert_eq!(
        recipe.as_str(),
        digest_b3_typed(&canonical_json_bytes(&rebuilt)?).as_str(),
    );
    let argv = UnitResultV1::argv_digest_from_manifest_entry(&entry)?;
    assert_eq!(
        argv.as_str(),
        digest_b3_typed(&canonical_json_bytes(&entry.argv)?).as_str(),
    );
    let input = UnitResultV1::effective_input_digest_from_manifest_entry(&entry)?;
    let rebuilt_input = EffectiveInputDigestInputs {
        env: &entry.env,
        matrix_id: entry.matrix_id.as_str(),
        matrix_key: entry.matrix_key.as_str(),
        matrix_max_parallel: entry.matrix_max_parallel,
    };
    assert_eq!(
        input.as_str(),
        digest_b3_typed(&canonical_json_bytes(&rebuilt_input)?).as_str(),
    );
    let mut changed = entry.clone();
    changed.argv.push("--offline".to_owned());
    assert_ne!(
        UnitResultV1::argv_digest_from_manifest_entry(&changed)?.as_str(),
        argv.as_str(),
        "argv digest tracks the real argv",
    );
    Ok(())
}

#[test]
fn group_digest_binds_group_and_plan() -> Result<(), ContractError> {
    let plan = UnitLedgerV1::plan_digest_from_canonical_bytes(b"gc1-t53-plan");
    let left = UnitLedgerV1::group_digest_for_group("my-group", &plan)?;
    let documented = GroupDigestInputs {
        group_key: "my-group",
        plan_digest: &plan,
    };
    assert_eq!(
        left.as_str(),
        digest_b3_typed(&canonical_json_bytes(&documented)?).as_str(),
    );
    assert_ne!(
        UnitLedgerV1::group_digest_for_group("other-group", &plan)?.as_str(),
        left.as_str(),
    );
    assert!(
        UnitLedgerV1::group_digest_for_group("", &plan).is_err(),
        "empty group key rejected"
    );
    Ok(())
}

#[test]
fn sidecar_digests_cover_bytes_or_absence() -> Result<(), ContractError> {
    let staged = br#"{"session":"gc1-t53"}"#;
    let observed = UnitLedgerV1::observation_digest_for_sidecars(&[Some(&staged[..]), None])?;
    let per_unit = vec![
        digest_b3_typed(staged).as_str().to_owned(),
        digest_b3_typed(UnitLedgerV1::ABSENT_OBSERVATION_SIDECAR_INPUT.as_bytes())
            .as_str()
            .to_owned(),
    ];
    assert_eq!(
        observed.as_str(),
        digest_b3_typed(&canonical_json_bytes(&per_unit)?).as_str(),
    );
    let binding = UnitLedgerV1::source_binding_digest_for_sidecars(&[None, None])?;
    assert_ne!(
        observed.as_str(),
        binding.as_str(),
        "observation and source-binding absence encodings differ",
    );
    assert!(
        UnitLedgerV1::sidecar_set_digest(&[None], "").is_err(),
        "empty absence encoding rejected"
    );
    Ok(())
}
