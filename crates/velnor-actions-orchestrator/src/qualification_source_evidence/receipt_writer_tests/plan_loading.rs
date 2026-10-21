#[test]
fn absent_or_malformed_plan_is_left_for_the_merge_failure_report() {
    let temp = tempfile::tempdir().expect("runner temp");
    assert!(
        super::super::load_receipt_plan("r1-a1", temp.path())
            .expect("missing plan is not a writer error")
            .is_none()
    );

    let run_dir = temp.path().join("velnor").join("r1-a1");
    std::fs::create_dir_all(&run_dir).expect("run directory");
    std::fs::write(run_dir.join("plan.json"), b"{").expect("malformed plan");
    assert!(
        super::super::load_receipt_plan("r1-a1", temp.path())
            .expect("malformed plan is not a writer error")
            .is_none()
    );
}

#[test]
fn valid_qualification_plan_still_requires_trusted_backend_snapshots() {
    let prepared = super::prepared_artifacts();
    let mut owners = vec![prepared.owner.clone()];
    let error = super::super::apply_backend_observations(
        &mut owners,
        &prepared.plan,
        &prepared.authenticated,
        prepared.temp.path(),
        &prepared.plan.run_key,
    )
    .expect_err("missing BEFORE evidence remains fail-closed");
    assert!(
        error
            .to_string()
            .contains("qualification_receipt_file_missing")
    );
}
