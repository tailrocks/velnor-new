use super::*;

#[test]
fn task_report_uses_the_exact_physical_job_lane() {
    let mut plan = fixture_plan();
    let mut paired = Vec::new();
    for entry in &plan.matrix.include {
        for variant in [
            NamedCheckLaneVariant::Hosted,
            NamedCheckLaneVariant::ScaleSet,
        ] {
            let (suffix, runner) = match variant {
                NamedCheckLaneVariant::Hosted => (
                    "__hosted",
                    PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")
                        .expect("hosted platform"),
                ),
                NamedCheckLaneVariant::ScaleSet => (
                    "__local",
                    PlannedPlatform::new(
                        "scale-set:velnor+ubuntu-26.04-scale-set",
                        "x86_64-unknown-linux-gnu",
                    )
                    .expect("scale-set platform"),
                ),
            };
            paired.push(
                MatrixEntry::derive_for_lane(
                    &entry.stack_id,
                    &entry.task_id,
                    &entry.run,
                    &entry.task_digest,
                    entry.adapter_metadata.clone(),
                    entry.execute_task_ids.clone(),
                    &entry.input_digest,
                    "local",
                    &format!("rust-demo{suffix}"),
                    Some(variant),
                    runner,
                )
                .expect("lane entry"),
            );
        }
    }
    plan.matrix.include = paired;
    plan.validate().expect("paired plan validates");
    let temp = staged_run(&plan, "local");
    assert_eq!(
        write_task_report_for_job("local", CLIPPY, 0, None, "rust-demo__local", temp.path())
            .expect("scale-set report"),
        1
    );
    let local = plan
        .matrix
        .include
        .iter()
        .find(|entry| {
            entry.task_id == CLIPPY && entry.lane_variant == Some(NamedCheckLaneVariant::ScaleSet)
        })
        .expect("scale-set matrix entry");
    let hosted = plan
        .matrix
        .include
        .iter()
        .find(|entry| {
            entry.task_id == CLIPPY && entry.lane_variant == Some(NamedCheckLaneVariant::Hosted)
        })
        .expect("hosted matrix entry");
    assert!(
        temp.path()
            .join("velnor/local")
            .join(&local.matrix_key)
            .exists()
    );
    assert!(
        !temp
            .path()
            .join("velnor/local")
            .join(&hosted.matrix_key)
            .exists()
    );
}

#[test]
fn unbound_inputs_fail_before_writing() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    for (run_key, task, exit) in [
        ("local", CLIPPY, 256),
        ("local", CLIPPY, -1),
        ("local", "stack/rust/demo/unknown/default", 0),
        ("r1-a1", CLIPPY, 0),
    ] {
        assert!(
            write_task_report_to(run_key, task, exit, None, temp.path()).is_err(),
            "must reject {run_key}/{task}/{exit}"
        );
    }
    let missing = TempDir::new().expect("tempdir");
    assert!(write_task_report_to("local", CLIPPY, 0, None, missing.path()).is_err());
    let corrupt = TempDir::new().expect("tempdir");
    let dir = corrupt.path().join("velnor").join("local");
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(dir.join("plan.json"), "not json").expect("plan file");
    assert!(write_task_report_to("local", CLIPPY, 0, None, corrupt.path()).is_err());
    let drifted = staged_run(&plan, "r1-a1");
    assert!(write_task_report_to("r1-a1", CLIPPY, 0, None, drifted.path()).is_err());
    let rewrite = staged_run(&plan, "local");
    write_task_report_to("local", CLIPPY, 0, None, rewrite.path()).expect("first write");
    assert!(write_task_report_to("local", CLIPPY, 0, None, rewrite.path()).is_err());
}

#[cfg(unix)]
#[test]
fn planted_symlink_at_report_path_refuses_without_writing() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let entry = &plan.matrix.include[0];
    let dir = temp
        .path()
        .join("velnor")
        .join("local")
        .join(&entry.matrix_key);
    let report_dir = dir.join("tasks");
    fs::create_dir_all(&report_dir).expect("report dirs");
    let loot = temp.path().join("loot.json");
    std::os::unix::fs::symlink(&loot, dir.join("matrix-report.json")).expect("plant");
    let err =
        write_task_report_to("local", CLIPPY, 0, None, temp.path()).expect_err("plant refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
    assert!(!loot.exists(), "producer bytes never followed the plant");
}

#[test]
fn ambiguous_entries_refuse_rather_than_guess() {
    let mut plan = fixture_plan();
    plan.matrix.include[0]
        .execute_task_ids
        .tasks
        .insert("extra".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()));
    let multi = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, multi.path()).is_err());

    let mut plan = fixture_plan();
    plan.matrix.include[0].execute_task_ids.tasks =
        BTreeMap::from([("only".to_owned(), ExecuteTaskRef::Single(TEST.to_owned()))]);
    let dupe = staged_run(&plan, "local");
    assert!(write_task_report_to("local", TEST, 0, None, dupe.path()).is_err());

    let mut plan = fixture_plan();
    plan.obligations.clear();
    plan.task_ids.clear();
    plan.validate().expect("obligation-free plan validates");
    let orphan = staged_run(&plan, "local");
    assert!(write_task_report_to("local", CLIPPY, 0, None, orphan.path()).is_err());
}
