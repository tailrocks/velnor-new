use super::*;

const SECOND_TASK_ID: &str = "stack/rust/demo/test/default";

fn grouped_fixture() -> (TempDir, Plan, Vec<MatrixEntry>) {
    let (temp, mut plan, first, _) = fixture();
    let task_digest = digest(4);
    let input_digest = digest(5);
    let closure_digest = digest(6);
    let second = MatrixEntry::derive(
        "rust",
        SECOND_TASK_ID,
        "true",
        &task_digest,
        serde_json::json!({"features": ["secondary"]}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "test".to_owned(),
                ExecuteTaskRef::Single(SECOND_TASK_ID.to_owned()),
            )]),
        },
        &input_digest,
        RUN_KEY,
        JOB_ID,
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform"),
    )
    .expect("second entry");
    plan.obligations.push(PlanObligation {
        job_id: JOB_ID.to_owned(),
        task_id: SECOND_TASK_ID.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: task_digest.clone(),
        task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(task_digest)
            .expect("binding digest"),
        input_digest,
        closure_digest,
        graph_digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        mbx_digest: "b3-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        profile: "default".to_owned(),
    });
    plan.matrix.include.push(second.clone());
    plan.matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    plan.task_ids.push(SECOND_TASK_ID.to_owned());
    plan.task_ids.sort();
    plan.validate().expect("grouped plan");
    (temp, plan, vec![first, second])
}

fn obligation_for<'a>(plan: &'a Plan, task_id: &str) -> &'a PlanObligation {
    plan.obligations
        .iter()
        .find(|obligation| obligation.task_id == task_id)
        .expect("task obligation")
}

fn grouped_context(
    temp: &TempDir,
    plan: &Plan,
) -> super::super::context::AuthenticatedProducerContext {
    let producer = QualificationCacheProducerContext {
        mode: QualificationDispatchMode::Controlled,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: plan.head.clone(),
        source_sha: plan.head.clone(),
        run: QualificationRunRef {
            run_id: RUN_ID,
            run_attempt: 1,
        },
    };
    super::super::context::AuthenticatedProducerContext {
        dispatch: plan.qualification.clone().expect("dispatch"),
        producer,
        runner_temp: temp.path().to_path_buf(),
        job_id: JOB_ID.to_owned(),
        run_key: RUN_KEY.to_owned(),
    }
}

#[test]
fn grouped_job_writes_one_bound_sidecar_for_each_logical_entry() {
    let (temp, plan, entries) = grouped_fixture();
    let selected = super::super::job_entries(&plan, JOB_ID).expect("job entries");
    assert_eq!(selected.len(), 2);
    for entry in &entries {
        write_reports(&temp, &plan, entry, 0);
    }
    let authenticated = grouped_context(&temp, &plan);
    let mut evidence = Vec::with_capacity(selected.len());

    for entry in selected {
        let obligation = obligation_for(&plan, &entry.task_id);
        let reports = read_successful_lane_reports(temp.path(), &plan, entry, &entry.task_digest)
            .expect("entry reports");
        let observed = ObservedLaneClosure {
            matrix_key: entry.matrix_key.clone(),
            task_id: entry.task_id.clone(),
            observed_task_digest: entry.task_digest.clone(),
            observed_input_digest: entry.input_digest.clone(),
            observed_closure_digest: obligation.closure_digest.clone(),
        };
        evidence.push((entry, observed, reports));
    }
    super::super::write_job_sidecars(
        temp.path(),
        &plan,
        &authenticated,
        &plan.generator,
        evidence,
    )
    .expect("group sidecars");

    for entry in &entries {
        let sidecar = temp
            .path()
            .join("velnor")
            .join(RUN_KEY)
            .join(&entry.matrix_key)
            .join(SOURCE_EVIDENCE_FILENAME);
        let document: QualificationSourceEvidenceDocument =
            serde_json::from_slice(&fs::read(sidecar).expect("sidecar bytes"))
                .expect("sidecar document");
        assert_eq!(document.evidence.job_id, JOB_ID);
        assert_eq!(document.evidence.task_id, entry.task_id);
        assert_eq!(document.evidence.matrix_key, entry.matrix_key);
        assert_eq!(document.evidence.planned_task_digest, entry.task_digest);
    }
}

#[test]
fn grouped_job_fails_when_one_logical_entry_report_is_missing() {
    let (temp, plan, entries) = grouped_fixture();
    write_reports(&temp, &plan, &entries[0], 0);

    let error =
        read_successful_lane_reports(temp.path(), &plan, &entries[1], &entries[1].task_digest)
            .expect_err("missing entry reports fail");
    assert!(
        error
            .to_string()
            .contains("qualification_source_report_missing")
    );
}

#[test]
fn grouped_job_rejects_duplicate_or_foreign_job_entries() {
    let (_, mut plan, entries) = grouped_fixture();
    plan.matrix.include.push(entries[0].clone());
    let duplicate = super::super::job_entries(&plan, JOB_ID).expect_err("duplicate fails");
    assert!(
        duplicate
            .to_string()
            .contains("qualification_source_job_entry_duplicate")
    );

    let foreign = super::super::job_entries(&plan, "qualification-rust-foreign")
        .expect_err("foreign job fails");
    assert!(
        foreign
            .to_string()
            .contains("qualification_source_job_missing")
    );

    plan.matrix.include.pop();
    let second = plan
        .matrix
        .include
        .iter_mut()
        .find(|entry| entry.matrix_key == entries[1].matrix_key)
        .expect("second entry");
    second.artifact_id = "foreign-artifact".to_owned();
    let mismatch = super::super::job_entries(&plan, JOB_ID).expect_err("foreign artifact fails");
    assert!(
        mismatch
            .to_string()
            .contains("qualification_source_job_entry_mismatch")
    );
}

#[test]
fn grouped_job_rejects_tampered_task_digest() {
    let (temp, mut plan, entries) = grouped_fixture();
    write_reports(&temp, &plan, &entries[0], 0);
    write_reports(&temp, &plan, &entries[1], 0);
    let index = plan
        .matrix
        .include
        .iter()
        .position(|entry| entry.matrix_key == entries[1].matrix_key)
        .expect("entry index");
    plan.matrix.include[index].task_digest = digest(9);

    let error = read_successful_lane_reports(
        temp.path(),
        &plan,
        &plan.matrix.include[index],
        &entries[1].task_digest,
    )
    .expect_err("tampered plan digest fails");
    assert!(
        error
            .to_string()
            .contains("qualification_source_task_plan_mismatch")
    );
}
