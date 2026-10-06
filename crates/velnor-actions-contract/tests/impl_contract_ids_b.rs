//! Contract matrix-entry derivation and validation cases.
use crate::impl_contract_ids::{GROUP, MANIFEST, SAMPLE_RUN, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ContractError, ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, PlannedPlatform, digest_b3,
    report_id_for_matrix, run_key_for_ci,
};

#[test]
fn matrix_entry_derives_and_validates() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(7, 2);
    let entry = sample_entry(&run_key)?;
    entry.validate(&run_key)?;
    assert_eq!(entry.id, format!("stack:rust|task:{GROUP}"));
    assert_eq!(
        entry.report_id,
        report_id_for_matrix(&run_key, &entry.matrix_key)?
    );
    let mut bad = entry.clone();
    bad.matrix_key = "m-0000000000000000".to_owned();
    assert!(bad.validate(&run_key).is_err());
    let single = entry.execute_task_ids.tasks.get("clippy");
    assert!(matches!(single, Some(ExecuteTaskRef::Single(id)) if id == TASK));
    Ok(())
}

#[test]
fn matrix_entry_carries_leg_command_and_digest() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(4242, 7);
    let entry = sample_entry(&run_key)?;
    assert_eq!(entry.run, SAMPLE_RUN);
    assert_eq!(entry.task_digest, digest_b3(b"task-bytes"));
    entry.validate(&run_key)?;
    let value = serde_json::to_value(&entry).expect("serialize");
    assert_eq!(value["run"], SAMPLE_RUN);
    let roundtrip: MatrixEntry = serde_json::from_value(value).expect("deserialize");
    roundtrip.validate(&run_key)?;
    Ok(())
}

#[test]
fn matrix_entry_rejects_bad_run_and_digest() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(4242, 7);
    let mut tasks = BTreeMap::new();
    tasks.insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
    let derive = |run: &str, digest: &str| {
        MatrixEntry::derive(
            "rust",
            GROUP,
            run,
            digest,
            serde_json::json!({"manifest": MANIFEST}),
            ExecuteTaskIds {
                tasks: tasks.clone(),
            },
            &digest_b3(b"entry-inputs"),
            &run_key,
            "plan",
            PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?,
        )
    };
    assert!(derive("", &digest_b3(b"task-bytes")).is_err());
    assert!(derive("a\nb", &digest_b3(b"task-bytes")).is_err());
    assert!(derive(SAMPLE_RUN, "bogus").is_err());
    let mut entry = sample_entry(&run_key)?;
    entry.run.clear();
    assert!(entry.validate(&run_key).is_err());
    Ok(())
}
