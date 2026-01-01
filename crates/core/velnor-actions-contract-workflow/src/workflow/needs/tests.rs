use super::*;
use crate::workflow::{JobOutput, Step, StepId, StepKind, StepRole, job_output::JobOutputName};
use velnor_actions_contract_config::config::ExecutionMode;

/// Conclusions over one unsorted inventory.
fn conclusions(inventory: &[&str]) -> NeedsConclusions {
    NeedsConclusions {
        required_job: "required".to_owned(),
        inventory: inventory.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn expected_env_emits_sorted_inventory_json() {
    let (key, value) = conclusions(&["zizmor", "plan", "alint"]).expected_env();
    assert_eq!(key, NEEDS_EXPECTED_ENV);
    assert_eq!(value, "[\"alint\",\"plan\",\"zizmor\"]");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&value).expect("valid json"),
        ["alint", "plan", "zizmor"],
    );
}

/// Minimal job with the given `needs` for inventory tests.
fn job_with_needs(needs: &[&str]) -> Job {
    Job {
        outputs: Vec::new(),
        display_name: "Test".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: crate::workflow::timeout::JobTimeout::VALIDATOR,
        needs: needs.iter().map(ToString::to_string).collect(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    }
}

#[test]
fn inventory_comes_from_gate_needs_not_job_set() {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), job_with_needs(&[]));
    jobs.insert("required".to_owned(), job_with_needs(&["plan"]));
    jobs.insert("publish-baseline".to_owned(), job_with_needs(&["required"]));
    jobs.insert("unrelated".to_owned(), job_with_needs(&[]));
    let conclusions =
        NeedsConclusions::from_finalized_jobs("required", &jobs).expect("gate with needs derives");
    assert_eq!(conclusions.inventory, vec!["plan".to_owned()]);
    assert!(conclusions.gate_matches(&jobs));
    let (_, value) = conclusions.expected_env();
    assert_eq!(value, "[\"plan\"]");
}

#[test]
fn inventory_rejects_missing_gate_and_empty_needs() {
    let mut jobs = BTreeMap::new();
    jobs.insert("plan".to_owned(), job_with_needs(&[]));
    let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
        panic!("missing gate fails");
    };
    assert!(err.to_string().contains("missing_gate"), "{err}");
    jobs.insert("required".to_owned(), job_with_needs(&[]));
    let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
        panic!("empty needs fail");
    };
    assert!(err.to_string().contains("empty_inventory"), "{err}");
}

#[test]
fn expected_env_handles_edge_inventories() {
    let (key, value) = conclusions(&["plan"]).expected_env();
    assert_eq!(
        (key.as_str(), value.as_str()),
        (NEEDS_EXPECTED_ENV, "[\"plan\"]")
    );
    let (_, value) = conclusions(&[]).expected_env();
    assert_eq!(value, "[]", "empty inventory stays explicit");
}

fn report_producer() -> Job {
    let mut job = job_with_needs(&[]);
    job.outputs = vec![
        JobOutput::task_report_artifact_id(),
        JobOutput::task_report_check_run_id(),
    ];
    job.steps.push(Step {
        name: "Upload report".to_owned(),
        id: Some(StepId::CrateReportUpload),
        role: Some(StepRole::CrateReportUpload),
        condition: None,
        kind: StepKind::Action {
            uses: "actions/upload-artifact@v4".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    });
    job
}

#[test]
fn non_both_mode_leaves_legacy_jobs_uninspected() {
    let mut jobs = BTreeMap::new();
    jobs.insert("required".to_owned(), job_with_needs(&["crate"]));
    let mut legacy = report_producer();
    legacy
        .outputs
        .retain(|output| output.name == JobOutputName::TaskReportArtifactId);
    jobs.insert("crate".to_owned(), legacy);
    assert_eq!(
        TaskReportProducerInventory::from_finalized_jobs(None, "required", &jobs)
            .expect("legacy route does not require the inventory"),
        None,
    );
    assert_eq!(
        TaskReportProducerInventory::from_finalized_jobs(
            Some(ExecutionMode::Hosted),
            "required",
            &jobs,
        )
        .expect("hosted route does not require the inventory"),
        None,
    );
}

#[test]
fn both_inventory_uses_retargeted_direct_producer_ids() {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "required".to_owned(),
        job_with_needs(&["crate__local", "ordinary", "crate__hosted"]),
    );
    jobs.insert("crate__local".to_owned(), report_producer());
    jobs.insert("crate__hosted".to_owned(), report_producer());
    jobs.insert("ordinary".to_owned(), job_with_needs(&[]));
    let hosted_outputs = jobs["crate__hosted"].outputs.clone();
    let local_outputs = jobs["crate__local"].outputs.clone();
    let inventory = TaskReportProducerInventory::from_finalized_jobs(
        Some(ExecutionMode::Both),
        "required",
        &jobs,
    )
    .expect("well-formed finalized route")
    .expect("report producers are present");
    assert_eq!(
        inventory.workflow_job_keys,
        ["crate__hosted", "crate__local"],
    );
    assert_eq!(jobs["crate__hosted"].outputs, hosted_outputs);
    assert_eq!(jobs["crate__local"].outputs, local_outputs);
    assert_eq!(
        inventory.expected_env().expect("inventory serializes"),
        (
            TASK_REPORT_PRODUCERS_EXPECTED_ENV.to_owned(),
            "[\"crate__hosted\",\"crate__local\"]".to_owned(),
        ),
    );
}

#[test]
fn both_inventory_rejects_missing_or_duplicate_direct_needs() {
    let mut jobs = BTreeMap::new();
    jobs.insert("required".to_owned(), job_with_needs(&["missing"]));
    let Err(error) = TaskReportProducerInventory::from_finalized_jobs(
        Some(ExecutionMode::Both),
        "required",
        &jobs,
    ) else {
        panic!("a missing finalized need must fail");
    };
    assert!(error.to_string().contains("missing_direct_need"));

    jobs.insert(
        "required".to_owned(),
        job_with_needs(&["crate__hosted", "crate__hosted"]),
    );
    jobs.insert("crate__hosted".to_owned(), report_producer());
    let Err(error) = TaskReportProducerInventory::from_finalized_jobs(
        Some(ExecutionMode::Both),
        "required",
        &jobs,
    ) else {
        panic!("a duplicated finalized need must fail");
    };
    assert!(error.to_string().contains("duplicate_direct_need"));
}

#[test]
fn both_inventory_rejects_partial_pairs_and_duplicate_report_bindings() {
    let mut jobs = BTreeMap::new();
    jobs.insert("required".to_owned(), job_with_needs(&["crate__hosted"]));
    let mut producer = report_producer();
    producer
        .outputs
        .retain(|output| output.name == JobOutputName::TaskReportArtifactId);
    jobs.insert("crate__hosted".to_owned(), producer);
    assert_inventory_error(&jobs, "incomplete_report_output_pair");

    let mut producer = report_producer();
    producer.outputs.push(JobOutput::task_report_artifact_id());
    jobs.insert("crate__hosted".to_owned(), producer);
    assert_inventory_error(&jobs, "incomplete_report_output_pair");

    let mut producer = report_producer();
    producer.outputs.push(JobOutput::task_report_check_run_id());
    jobs.insert("crate__hosted".to_owned(), producer);
    assert_inventory_error(&jobs, "incomplete_report_output_pair");
}

#[test]
fn both_inventory_rejects_duplicate_upload_roles_and_hidden_producers() {
    let mut jobs = BTreeMap::new();
    jobs.insert("required".to_owned(), job_with_needs(&["crate__hosted"]));
    let mut duplicate_upload = report_producer();
    duplicate_upload
        .steps
        .push(duplicate_upload.steps[0].clone());
    jobs.insert("crate__hosted".to_owned(), duplicate_upload);
    assert_inventory_error(&jobs, "incomplete_report_output_pair");

    let mut duplicate_upload_id = report_producer();
    duplicate_upload_id.steps.push(Step {
        name: "Unclassified second upload identity".to_owned(),
        id: Some(StepId::CrateReportUpload),
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    });
    jobs.insert("crate__hosted".to_owned(), duplicate_upload_id);
    assert_inventory_error(&jobs, "incomplete_report_output_pair");

    jobs.insert("crate__hosted".to_owned(), report_producer());
    jobs.insert("hidden".to_owned(), report_producer());
    assert_inventory_error(&jobs, "report_producer_not_direct_need");

    let mut hidden_by_id = job_with_needs(&[]);
    hidden_by_id.steps.push(Step {
        name: "Unclassified report upload".to_owned(),
        id: Some(StepId::CrateReportUpload),
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    });
    jobs.insert("hidden".to_owned(), hidden_by_id);
    assert_inventory_error(&jobs, "report_producer_not_direct_need");
}

fn assert_inventory_error(jobs: &BTreeMap<String, Job>, expected: &str) {
    let Err(error) = TaskReportProducerInventory::from_finalized_jobs(
        Some(ExecutionMode::Both),
        "required",
        jobs,
    ) else {
        panic!("malformed report-producer graph must fail");
    };
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn plan_format_report_producer_is_included_without_mutating_its_outputs() {
    let mut plan = report_producer();
    plan.steps.push(Step {
        name: "Format workspace".to_owned(),
        id: None,
        role: Some(StepRole::PlanFormat),
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "mise".to_owned(),
                "run".to_owned(),
                "format:check".to_owned(),
            ],
            env: BTreeMap::new(),
        },
    });
    let original_outputs = plan.outputs.clone();
    let mut jobs = BTreeMap::new();
    jobs.insert("required".to_owned(), job_with_needs(&["plan"]));
    jobs.insert("plan".to_owned(), plan);

    let inventory = TaskReportProducerInventory::from_finalized_jobs(
        Some(ExecutionMode::Both),
        "required",
        &jobs,
    )
    .expect("well-formed finalized Plan-format graph")
    .expect("Plan's format report producer is included");

    assert_eq!(inventory.workflow_job_keys, ["plan"]);
    assert_eq!(jobs["plan"].outputs, original_outputs);
}
