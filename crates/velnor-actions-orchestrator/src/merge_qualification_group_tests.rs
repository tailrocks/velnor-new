use super::*;

#[test]
fn a_shared_rust_job_keeps_each_task_report_required() {
    let plan = shared_rust_plan();
    let mut request = request(Some(context()));
    request.required_job_ids = vec!["plan".to_owned(), "rust-demo".to_owned()];
    assert_shared_owner_is_required(&plan, &request);

    let (covered, signals, miss_reasons) =
        cover_shared_entry_with_one_task_missing(&plan, &request);
    assert!(!covered);
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("source_missing"));
    assert_eq!(
        crate::merge::decide(&signals, &plan),
        FinalStatus::PlanningFailed
    );
}

fn assert_shared_owner_is_required(plan: &Plan, request: &MergeRequest) {
    let entry = &plan.matrix.include[0];
    assert_eq!(entry.job_id, "rust-demo");
    assert_eq!(entry.execute_task_ids.tasks.len(), 2);
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();
    check_matrix_job_inventory(plan, request, &mut signals, &mut reasons);
    assert!(!signals.planning_failed, "{reasons:?}");
}

fn cover_shared_entry_with_one_task_missing(
    plan: &Plan,
    request: &MergeRequest,
) -> (bool, Signals, BTreeSet<String>) {
    let entry = &plan.matrix.include[0];
    let obligation_digests = plan_digests(plan);
    let clippy_digest = obligation_digests[CLIPPY].to_owned();
    let clippy_report_id =
        task_report_id_for_task(&request.run_key, &entry.matrix_key, &clippy_digest)
            .expect("clippy report ID");
    let test_report_id = task_report_id_for_task(
        &request.run_key,
        &entry.matrix_key,
        obligation_digests[TEST],
    )
    .expect("test report ID");
    let matrix_report =
        shared_matrix_report(entry, &request.run_key, &clippy_report_id, &test_report_id);
    let clippy_report = task_report(
        &request.run_key,
        entry,
        CLIPPY,
        &clippy_digest,
        &clippy_report_id,
    );
    let task_files = BTreeMap::from([(clippy_report_id.as_str(), &clippy_report)]);
    let mut seen_task_reports = BTreeSet::new();
    let mut fold = Fold::default();
    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    let covered = {
        let mut sinks = CoverSinks {
            seen_task_reports: &mut seen_task_reports,
            task_files: &task_files,
            fold: &mut fold,
            signals: &mut signals,
            miss_reasons: &mut miss_reasons,
        };
        cover_entry(
            request,
            entry,
            &matrix_report,
            &obligation_digests,
            &mut sinks,
        )
        .expect("merge coverage")
    };
    (covered, signals, miss_reasons)
}

fn shared_matrix_report(
    entry: &MatrixEntry,
    run_key: &str,
    clippy_report_id: &str,
    test_report_id: &str,
) -> MatrixReport {
    MatrixReport {
        schema: 1,
        report_id: entry.report_id.clone(),
        run_key: run_key.to_owned(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![CLIPPY.to_owned(), TEST.to_owned()],
        task_report_ids: vec![clippy_report_id.to_owned(), test_report_id.to_owned()],
        tasks: vec![
            MatrixTaskEntry {
                task_report_id: clippy_report_id.to_owned(),
                task_id: CLIPPY.to_owned(),
                status: TaskStatus::Executed,
                exit_code: 0,
            },
            MatrixTaskEntry {
                task_report_id: test_report_id.to_owned(),
                task_id: TEST.to_owned(),
                status: TaskStatus::Executed,
                exit_code: 0,
            },
        ],
        selected: 2,
        reused: 0,
        executed: 2,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    }
}

#[test]
fn event_inventory_keeps_shared_rust_jobs_and_routes_both_lanes() {
    let rust_jobs = ["rust-demo__hosted", "rust-demo__local"];
    let tofu_jobs = ["tofu-demo__hosted", "tofu-demo__local"];
    let qualification_jobs = [
        "tofu-qualification-0123456789abcdef__hosted",
        "tofu-qualification-0123456789abcdef__local",
    ];
    let check_jobs = ["check-frontend__hosted", "check-frontend__local"];
    let mut all_ids = vec!["plan"];
    all_ids.extend(rust_jobs);
    all_ids.extend(tofu_jobs);
    all_ids.extend(qualification_jobs);
    all_ids.extend(check_jobs);
    all_ids.sort_unstable();

    let (ordinary, ordinary_job_results, ordinary_errors) = needs_inventory_for(
        "push",
        &all_ids,
        &rust_jobs,
        &tofu_jobs,
        &qualification_jobs,
        &check_jobs,
        false,
    );
    let (qualification, qualification_job_results, qualification_errors) = needs_inventory_for(
        "workflow_dispatch",
        &all_ids,
        &rust_jobs,
        &tofu_jobs,
        &qualification_jobs,
        &check_jobs,
        true,
    );

    let mut expected_ordinary = vec!["plan".to_owned()];
    expected_ordinary.extend(rust_jobs.map(str::to_owned));
    expected_ordinary.extend(tofu_jobs.map(str::to_owned));
    expected_ordinary.extend(check_jobs.map(str::to_owned));
    expected_ordinary.sort();
    let mut expected_qualification = vec!["plan".to_owned()];
    expected_qualification.extend(rust_jobs.map(str::to_owned));
    expected_qualification.extend(qualification_jobs.map(str::to_owned));
    expected_qualification.extend(check_jobs.map(str::to_owned));
    expected_qualification.sort();

    assert_eq!(ordinary, expected_ordinary);
    assert_eq!(ordinary_job_results, expected_ordinary);
    assert!(ordinary_errors.is_empty(), "{ordinary_errors:?}");
    assert_eq!(qualification, expected_qualification);
    assert_eq!(qualification_job_results, expected_qualification);
    assert!(qualification_errors.is_empty(), "{qualification_errors:?}");
}

fn shared_rust_plan() -> Plan {
    let clippy_digest = digest(1);
    let test_digest = digest(2);
    let entry = MatrixEntry::derive(
        "rust",
        CLIPPY,
        "true",
        &digest(3),
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([
                (
                    "clippy".to_owned(),
                    ExecuteTaskRef::Single(CLIPPY.to_owned()),
                ),
                ("test".to_owned(), ExecuteTaskRef::Single(TEST.to_owned())),
            ]),
        },
        &digest(4),
        "r123-a1",
        "rust-demo",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("shared Rust entry");
    let mut plan = qualification_plan(context());
    plan.obligations = vec![
        obligation(CLIPPY, &clippy_digest, 1),
        obligation(TEST, &test_digest, 2),
    ];
    plan.task_ids = vec![CLIPPY.to_owned(), TEST.to_owned()];
    plan.matrix.include = vec![entry];
    plan.jobs = vec![velnor_actions_contract::PlanJob {
        job_id: "rust-demo".to_owned(),
        disposition: velnor_actions_contract::JobDisposition::Run,
    }];
    plan.validate().expect("shared Rust plan");
    plan
}

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn obligation(task_id: &str, task_digest: &str, seed: u8) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: task_digest.to_owned(),
        task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(
            task_digest.to_owned(),
        )
        .expect("binding digest"),
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        graph_digest: "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        mbx_digest: "b3-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        profile: "default".to_owned(),
        job_id: "rust-demo".to_owned(),
    }
}

fn task_report(
    run_key: &str,
    entry: &MatrixEntry,
    task_id: &str,
    task_digest: &str,
    report_id: &str,
) -> TaskReport {
    TaskReport {
        schema: 3,
        task_report_id: report_id.to_owned(),
        run_key: run_key.to_owned(),
        event: WorkflowEvent::Qualification,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: task_id.to_owned(),
        task_digest: task_digest.to_owned(),
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "task-key".to_owned(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        platform_binding: PlatformBinding::Unavailable {
            planned_platform_id: entry.planned_platform.platform_id.clone(),
            runner_environment: PlatformRunnerEnvironment::GithubHosted,
            reason: velnor_actions_contract::PlatformUnavailableReason::ObservationNotRecorded,
        },
        exit_code: 0,
        duration_ms: None,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    }
}

fn needs_inventory_for(
    event_name: &str,
    all_ids: &[&str],
    rust_jobs: &[&str],
    tofu_jobs: &[&str],
    qualification_jobs: &[&str],
    check_jobs: &[&str],
    is_qualification: bool,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut needs = BTreeMap::from([("plan".to_owned(), "success".to_owned())]);
    for job_id in rust_jobs.iter().chain(check_jobs) {
        needs.insert((*job_id).to_owned(), "success".to_owned());
    }
    for job_id in tofu_jobs {
        needs.insert(
            (*job_id).to_owned(),
            if is_qualification {
                "skipped"
            } else {
                "success"
            }
            .to_owned(),
        );
    }
    for job_id in qualification_jobs {
        needs.insert(
            (*job_id).to_owned(),
            if is_qualification {
                "success"
            } else {
                "skipped"
            }
            .to_owned(),
        );
    }
    let needs_json = serde_json::to_string(&needs).expect("needs JSON");
    let expected_json = serde_json::to_string(all_ids).expect("expected IDs JSON");
    let temp = tempfile::TempDir::new().expect("temporary run directory");
    let request = crate::merge_request::assemble_with_needs(
        "r123-a1",
        temp.path(),
        Some(&needs_json),
        Some(&expected_json),
        Some(event_name),
        Some("{}"),
    )
    .expect("assemble needs inventory");
    let value: serde_json::Value = serde_json::from_str(&request).expect("merge request JSON");
    let inventory = value["required_job_ids"]
        .as_array()
        .expect("required job IDs")
        .iter()
        .map(|job_id| job_id.as_str().expect("job ID").to_owned())
        .collect();
    let jobs = value["required_jobs"]
        .as_array()
        .expect("required job results")
        .iter()
        .map(|job| job["job_id"].as_str().expect("job ID").to_owned())
        .collect();
    let errors = value["assembly_errors"]
        .as_array()
        .expect("assembly errors")
        .iter()
        .map(|error| error.as_str().expect("assembly error").to_owned())
        .filter(|error| {
            error == "needs_inventory_mismatch"
                || error.starts_with("inactive_crate_job_not_skipped:")
        })
        .collect();
    (inventory, jobs, errors)
}
