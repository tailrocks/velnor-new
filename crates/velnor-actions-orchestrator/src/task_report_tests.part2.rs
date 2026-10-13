#[test]
fn ambiguous_entries_refuse_rather_than_guess() {
    let mut plan = fixture_plan();
    plan.matrix.include[0]
        .execute_task_ids
        .tasks
        .insert("extra".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()));
    let multi = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], multi.path()).is_err());

    let mut plan = fixture_plan();
    plan.matrix.include[0].execute_task_ids.tasks =
        BTreeMap::from([("only".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()))]);
    let dupe = staged_run(&plan, "local");
    assert!(write_task_report_to("local", TEST, 0, None, &[], dupe.path()).is_err());

    let mut plan = fixture_plan();
    plan.obligations.clear();
    plan.task_ids.clear();
    plan.validate().expect("obligation-free plan validates");
    let orphan = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, &[], orphan.path()).is_err());
}
