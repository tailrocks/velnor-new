use super::*;

#[test]
fn matrix_and_report_ids_roundtrip() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(123, 1);
    assert_eq!(run_key, "r123-a1");
    validate_run_key("local")?;
    assert!(validate_run_key("r1").is_err());
    let id = matrix_id_for_task_group("rust", GROUP)?;
    validate_id(&id)?;
    assert!(validate_id("STACK:rust|task:x").is_err());
    let key = matrix_key_for_id(&id)?;
    validate_matrix_key(&key)?;
    assert!(key.starts_with("m-") && key.len() == 18);
    let report = report_id_for_matrix(&run_key, &key)?;
    validate_report_id(&report)?;
    let task_digest = digest_b3(b"task");
    let task_report = task_report_id_for_task(&run_key, &key, &task_digest)?;
    validate_task_report_id(&task_report)?;
    assert!(task_report.ends_with(&task_digest[3..19]));
    assert_eq!(plan_id_for_run(&run_key)?, "plan-r123-a1");
    validate_plan_id("plan-local")?;
    assert_eq!(
        target_key("x86_64-unknown-linux-gnu")?,
        "x86-64-unknown-linux-gnu"
    );
    assert_eq!(target_key("aarch64-apple-darwin")?, "aarch64-apple-darwin");
    Ok(())
}

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
