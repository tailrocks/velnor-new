// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::ir::WRITER_QUEUE_CANCEL_IN_PROGRESS;
use velnor_actions_contract::workflow::{LaneClass, lane_class};
use velnor_actions_contract::{
    CacheMode, Concurrency, DispatchInput, DispatchInputType, Job, JobDisposition, JobTimeout,
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NeedsConclusions,
    ObligationDecision, PermissionLevel, Permissions, Plan, PlanGenerator, PlanJob, PlanMatrix,
    PlanObligation, PlanRunner, REQUIRED_CONDITION, REQUIRED_GATE_CONDITION,
    REQUIRED_GATE_FAIL_CONDITION, REQUIRED_JOB_ID, RunnerSelection, RunsOn, ScheduleTrigger, Step,
    StepKind, Trigger, Trust, WorkflowDispatch, WorkflowEvent, WorkflowIr, plan_skips_work,
    selects_affected_work, trust_for_event,
};
use velnor_actions_workflow_renderer::cache_cleanup::{
    CACHE_CLEANUP_PATH, render_cache_cleanup_yml,
};
use velnor_actions_workflow_renderer::steps::{
    BOOTSTRAP_EXPECTED_KEY_ENV, BOOTSTRAP_MATCHED_KEY_ENV, BOOTSTRAP_MAX_BYTES,
    BOOTSTRAP_MAX_BYTES_ENV, BOOTSTRAP_MISS_CODE, BOOTSTRAP_SHADOW_CLEAR, BOOTSTRAP_SHADOW_ENV,
    BootstrapBridge, CACHE_BOOTSTRAP_SCRIPT, bootstrap_bridge_step,
};
use velnor_actions_workflow_renderer::{
    FINAL_CONDITION, FINAL_JOB_ID, MAX_WORKFLOW_BYTES,
    overlap::{PrepOverlap, wire_prep_join},
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn shell_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "printf ok".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn job_with(
    display: &str,
    cache_mode: Option<CacheMode>,
    permissions: Option<Permissions>,
    needs: Vec<String>,
    condition: Option<String>,
) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs,
        condition,
        permissions,
        cache_mode,
        environment: None,
        services: Vec::new(),
        steps: vec![shell_step(display)],
    }
}

fn cleanup_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Write,
        packages: PermissionLevel::None,
    }
}

fn mode_jobs(writer_mode: Option<CacheMode>) -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            "reader".to_owned(),
            job_with(
                "Reader",
                Some(CacheMode::explicit_read()),
                None,
                Vec::new(),
                None,
            ),
        ),
        (
            "writer".to_owned(),
            job_with("Writer", writer_mode, None, Vec::new(), None),
        ),
        (
            "cleanup".to_owned(),
            job_with(
                "Cleanup",
                None,
                Some(cleanup_permissions()),
                Vec::new(),
                None,
            ),
        ),
        (
            REQUIRED_JOB_ID.to_owned(),
            job_with(
                "Required",
                None,
                None,
                vec!["writer".to_owned(), "reader".to_owned()],
                Some(REQUIRED_CONDITION.to_owned()),
            ),
        ),
    ])
}

fn mode_workflow(trigger: Trigger, writer_mode: Option<CacheMode>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: trigger,
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "velnor-${{ github.workflow }}-${{ github.ref }}".to_owned(),
            cancel_in_progress: "${{ github.event_name == 'pull_request' }}".to_owned(),
        },
        jobs: mode_jobs(writer_mode),
    }
}

fn pr_trigger() -> Trigger {
    Trigger {
        pull_request_types: vec!["opened".to_owned(), "synchronize".to_owned()],
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn push_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: vec!["main".to_owned()],
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn merge_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: true,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn dispatch_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: Some(WorkflowDispatch {
            inputs: vec![DispatchInput {
                name: "environment".to_owned(),
                required: true,
                input_type: DispatchInputType::String,
                choices: Vec::new(),
                default: None,
            }],
        }),
        schedule: None,
    }
}

fn schedule_trigger() -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: None,
        schedule: Some(ScheduleTrigger::single("0 6 * * 1")),
    }
}

fn names<'a>(values: &'a [&'a str]) -> BTreeSet<&'a str> {
    values.iter().copied().collect()
}

fn check_required_inventory(
    jobs: &BTreeMap<String, Job>,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let conclusions = NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, jobs)
        .map_err(|error| fail(&format!("{label} lost its required inventory:{error}")))?;
    if conclusions.inventory != vec!["reader".to_owned(), "writer".to_owned()] {
        return Err(fail(&format!(
            "{label} dropped a selected unit from required:{:?}",
            conclusions.inventory
        )));
    }
    if !conclusions.gate_matches(jobs) {
        return Err(fail(&format!(
            "{label} left required detached from its jobs"
        )));
    }
    let (channel_key, channel_value) = conclusions.channel_env();
    if channel_key != NEEDS_CHANNEL_ENV || channel_value != NEEDS_CHANNEL_EXPRESSION {
        return Err(fail(&format!("{label} lost its needs channel")));
    }
    let (expected_key, expected_value) = conclusions.expected_env();
    if expected_key != NEEDS_EXPECTED_ENV || expected_value != "[\"reader\",\"writer\"]" {
        return Err(fail(&format!(
            "{label} lost its expected ledger:{expected_key}:{expected_value}"
        )));
    }
    Ok(())
}

fn check_mode(
    label: &str,
    event: WorkflowEvent,
    selects: bool,
    trusted: bool,
    trigger: Trigger,
    writer_mode: Option<CacheMode>,
) -> Result<(), Box<dyn Error>> {
    if selects_affected_work(event) != selects {
        return Err(fail(&format!("{label} left its affected-work selection")));
    }
    if trust_for_event(event) != if trusted { Trust::Trusted } else { Trust::Pr } {
        return Err(fail(&format!("{label} left its event trust")));
    }
    let workflow = mode_workflow(trigger, writer_mode);
    workflow
        .validate()
        .map_err(|error| fail(&format!("{label} rejected its event workflow:{error}")))?;
    workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .map_err(|error| fail(&format!("{label} rejected its event authority:{error}")))?;
    check_required_inventory(&workflow.jobs, label)?;
    Ok(())
}

fn check_event_modes() -> Result<(), Box<dyn Error>> {
    check_mode(
        "pr",
        WorkflowEvent::PullRequest,
        true,
        false,
        pr_trigger(),
        Some(CacheMode::push_write_expression()),
    )?;
    check_mode(
        "fork",
        WorkflowEvent::Fork,
        true,
        false,
        pr_trigger(),
        Some(CacheMode::push_write_expression()),
    )?;
    check_mode(
        "main",
        WorkflowEvent::Push,
        false,
        true,
        push_trigger(),
        Some(CacheMode::push_write_expression()),
    )?;
    check_mode(
        "merge-queue",
        WorkflowEvent::MergeGroup,
        false,
        false,
        merge_trigger(),
        Some(CacheMode::push_write_expression()),
    )?;
    check_mode(
        "tag",
        WorkflowEvent::Push,
        false,
        true,
        push_trigger(),
        None,
    )?;
    check_mode(
        "preview",
        WorkflowEvent::Local,
        true,
        false,
        dispatch_trigger(),
        None,
    )?;
    check_mode(
        "release",
        WorkflowEvent::Qualification,
        false,
        false,
        schedule_trigger(),
        None,
    )?;
    Ok(())
}

fn plan_with(dispositions: &[(&str, JobDisposition)]) -> Plan {
    Plan {
        schema: 2,
        run_key: "run".to_owned(),
        plan_id: "plan".to_owned(),
        base: None,
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-24.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "0.1.55".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "0".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: "task".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "selected".to_owned(),
            task_digest: "digest".to_owned(),
            input_digest: "digest".to_owned(),
            closure_digest: "digest".to_owned(),
            job_id: "job".to_owned(),
        }],
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: vec!["task".to_owned()],
        jobs: dispositions
            .iter()
            .map(|(id, disposition)| PlanJob {
                job_id: (*id).to_owned(),
                disposition: *disposition,
            })
            .collect(),
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn check_skipped_producers() -> Result<(), Box<dyn Error>> {
    if !plan_skips_work(&plan_with(&[
        ("a", JobDisposition::Run),
        ("b", JobDisposition::NotAffected),
    ])) {
        return Err(fail("skipped producer lost its skip signal"));
    }
    if plan_skips_work(&plan_with(&[("a", JobDisposition::Run)])) {
        return Err(fail("full plan kept a skip signal it never earned"));
    }
    if FINAL_CONDITION != REQUIRED_CONDITION || FINAL_CONDITION != "${{ !cancelled() }}" {
        return Err(fail("required verdict left its skipped-producer condition"));
    }
    if FINAL_JOB_ID != REQUIRED_JOB_ID || FINAL_JOB_ID != "required" {
        return Err(fail("required verdict left its job identity"));
    }
    Ok(())
}

fn check_hard_writer_failures() -> Result<(), Box<dyn Error>> {
    if REQUIRED_GATE_CONDITION != "${{ always() }}" {
        return Err(fail("required gate left its always condition"));
    }
    require(
        REQUIRED_GATE_FAIL_CONDITION,
        "needs.required.result != 'success'",
        "required gate",
    )?;
    require(REQUIRED_GATE_FAIL_CONDITION, "always()", "required gate")?;
    Ok(())
}

fn check_mixed_attempt_ledgers() -> Result<(), Box<dyn Error>> {
    let mut jobs = mode_jobs(Some(CacheMode::push_write_expression()));
    check_required_inventory(&jobs, "mixed-attempt")?;
    if let Some(required) = jobs.get_mut(REQUIRED_JOB_ID) {
        required.needs = vec!["reader".to_owned()];
    }
    let narrowed = NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, &jobs)
        .map_err(|error| fail(&format!("rerun ledger lost its inventory:{error}")))?;
    if narrowed.inventory != vec!["reader".to_owned()] {
        return Err(fail("rerun ledger kept a superseded producer"));
    }
    if narrowed.gate_matches(&mode_jobs(Some(CacheMode::push_write_expression()))) {
        return Err(fail("rerun ledger matched a stale producer set"));
    }
    Ok(())
}

fn guarded_bridge() -> BootstrapBridge {
    BootstrapBridge {
        payload_dir: "${{ runner.temp }}/velnor-gc1/transport/tools".to_owned(),
        member: "velnor-controller/velnor-actions".to_owned(),
        staged_path: "${{ runner.temp }}/velnor-gc1/staged/velnor-actions".to_owned(),
        max_bytes: BOOTSTRAP_MAX_BYTES,
        expected_key: Some("velnor-gc1-tools-group-compatibility-coverage-r0".to_owned()),
        matched_key: Some("velnor-gc1-tools-group-compatibility-coverage-r0".to_owned()),
        shadow_state: Some(BOOTSTRAP_SHADOW_CLEAR.to_owned()),
    }
}

fn check_shadow_guard() -> Result<(), Box<dyn Error>> {
    if BOOTSTRAP_MISS_CODE != 10 {
        return Err(fail("bootstrap miss left its cold-path exit code"));
    }
    if BOOTSTRAP_MAX_BYTES != 64 * 1024 * 1024 {
        return Err(fail("bootstrap bridge left its byte bound"));
    }
    if BOOTSTRAP_SHADOW_CLEAR != "clear" {
        return Err(fail("bootstrap shadow left its clear state"));
    }
    require(
        CACHE_BOOTSTRAP_SCRIPT,
        "matched_key != expected_key",
        "shadow guard",
    )?;
    require(CACHE_BOOTSTRAP_SCRIPT, "cold_miss:shadow", "shadow guard")?;
    let step = bootstrap_bridge_step(&guarded_bridge(), &"e".repeat(64))
        .map_err(|error| fail(&format!("shadow guard rejected its bridge:{error}")))?;
    let StepKind::Shell { env, .. } = &step.kind else {
        return Err(fail("shadow guard left its shell step"));
    };
    for (key, marker) in [
        (BOOTSTRAP_EXPECTED_KEY_ENV, "expected key"),
        (BOOTSTRAP_MATCHED_KEY_ENV, "matched key"),
        (BOOTSTRAP_SHADOW_ENV, "shadow state"),
        (BOOTSTRAP_MAX_BYTES_ENV, "byte bound"),
    ] {
        if !env.contains_key(key) {
            return Err(fail(&format!("shadow guard lost its {marker}")));
        }
    }
    Ok(())
}

fn check_denied_saves_repair_cleanup_lag() -> Result<(), Box<dyn Error>> {
    let rendered = render_cache_cleanup_yml("ubuntu-24.04", "0.1.55")
        .map_err(|error| fail(&format!("cleanup render rejected its lag job:{error}")))?;
    if rendered.path != CACHE_CLEANUP_PATH {
        return Err(fail("cleanup render left its workflow path"));
    }
    for marker in [
        "cancel-in-progress: false",
        "actions: write",
        "gh cache delete",
    ] {
        require(&rendered.bytes, marker, "cleanup lag")?;
    }
    forbid(&rendered.bytes, "actions/checkout", "cleanup lag")?;
    let queue =
        Concurrency::writer_queue("velnor-cache-writer-${{ github.repository }}".to_owned());
    if !queue.is_writer_queue() {
        return Err(fail("writer queue lost its queue identity"));
    }
    if queue.cancel_in_progress != WRITER_QUEUE_CANCEL_IN_PROGRESS
        || queue.cancel_in_progress != "false"
    {
        return Err(fail("writer queue kept a cancelling repair policy"));
    }
    queue
        .validate_writer_queue()
        .map_err(|error| fail(&format!("writer queue rejected its repair lane:{error}")))?;
    Ok(())
}

fn branch_jobs() -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            "download".to_owned(),
            job_with("Download", None, None, Vec::new(), None),
        ),
        (
            "image".to_owned(),
            job_with("Image", None, None, Vec::new(), None),
        ),
        (
            "join".to_owned(),
            job_with(
                "Join",
                None,
                None,
                Vec::new(),
                Some(REQUIRED_CONDITION.to_owned()),
            ),
        ),
    ])
}

fn check_static_branching_ledgers_platform_limits() -> Result<(), Box<dyn Error>> {
    let mut jobs = branch_jobs();
    wire_prep_join(
        &mut jobs,
        &PrepOverlap {
            download_job: "download".to_owned(),
            image_job: "image".to_owned(),
            join_job: "join".to_owned(),
        },
    )
    .map_err(|error| fail(&format!("static branch rejected its join:{error}")))?;
    let Some(join) = jobs.get("join") else {
        return Err(fail("static branch lost its join job"));
    };
    if !join.needs.contains(&"download".to_owned()) || !join.needs.contains(&"image".to_owned()) {
        return Err(fail("static branch left a ledger unwired"));
    }
    for (id, class) in [
        ("plan", LaneClass::Control),
        (REQUIRED_JOB_ID, LaneClass::Control),
        ("required-gate", LaneClass::Control),
        ("compare", LaneClass::Control),
        ("queue-monitor", LaneClass::Control),
        ("release-notes", LaneClass::SingleWriter),
        ("deploy-prod", LaneClass::SingleWriter),
        ("check-lint", LaneClass::Verification),
        ("jactionlint", LaneClass::Control),
    ] {
        if lane_class(id) != class {
            return Err(fail(&format!("static branch misclassified its lane:{id}")));
        }
    }
    if MAX_WORKFLOW_BYTES != 500_000 {
        return Err(fail("platform limit left its workflow byte ceiling"));
    }
    if !RunsOn::parse("ubuntu-24.04")?.is_scale_set()
        && RunsOn::parse("ubuntu-24.04")?.hosted_label() != Some("ubuntu-24.04")
    {
        return Err(fail("platform limit lost its hosted runner"));
    }
    if !RunsOn::parse("scale-set:velnor+my-set")?.is_scale_set() {
        return Err(fail("platform limit lost its scale-set runner"));
    }
    if RunsOn::parse("ubuntu-latest").is_ok() {
        return Err(fail("platform limit accepted a floating runner"));
    }
    Ok(())
}

#[test]
fn gc1_workflow_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_event_modes()?;
    check_skipped_producers()?;
    check_hard_writer_failures()?;
    check_mixed_attempt_ledgers()?;
    check_shadow_guard()?;
    check_denied_saves_repair_cleanup_lag()?;
    check_static_branching_ledgers_platform_limits()?;
    Ok(())
}

fn check_rejects_untrusted_event_writes() -> Result<(), Box<dyn Error>> {
    if mode_workflow(pr_trigger(), Some(CacheMode::Write))
        .validate()
        .is_ok()
    {
        return Err(fail("untrusted event path accepted an unconditional write"));
    }
    let mut workflow = mode_workflow(pr_trigger(), None);
    if let Some(reader) = workflow.jobs.get_mut("reader") {
        reader.cache_mode = None;
    }
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check accepted a reader without explicit read",
        ));
    }
    Ok(())
}

fn checkout_step() -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn check_rejects_authority_mismatch() -> Result<(), Box<dyn Error>> {
    let mut jobs = mode_jobs(None);
    if let Some(cleanup) = jobs.get_mut("cleanup") {
        cleanup.steps.insert(0, checkout_step());
    }
    let mut workflow = mode_workflow(pr_trigger(), None);
    workflow.jobs = jobs;
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check let cleanup hold an application checkout",
        ));
    }
    let mut jobs = mode_jobs(None);
    jobs.insert(
        "extra".to_owned(),
        job_with("Extra", None, Some(cleanup_permissions()), Vec::new(), None),
    );
    let mut workflow = mode_workflow(pr_trigger(), None);
    workflow.jobs = jobs;
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check let a non-cleanup job hold actions write",
        ));
    }
    let mut restricted = pr_trigger();
    restricted.pull_request_types = vec!["pull_request_target".to_owned()];
    if mode_workflow(restricted, None).validate().is_ok() {
        return Err(fail(
            "event path accepted a restricted untrusted actor event",
        ));
    }
    let mut scheduled = schedule_trigger();
    scheduled.schedule = Some(ScheduleTrigger::single("not-a-cron"));
    if mode_workflow(scheduled, None).validate().is_ok() {
        return Err(fail("event path accepted a malformed schedule"));
    }
    Ok(())
}

fn check_rejects_shadow_confusion() -> Result<(), Box<dyn Error>> {
    let sha = "e".repeat(64);
    let dangling_matched = BootstrapBridge {
        expected_key: None,
        ..guarded_bridge()
    };
    if bootstrap_bridge_step(&dangling_matched, &sha).is_ok() {
        return Err(fail(
            "shadow guard accepted a matched key without its expected key",
        ));
    }
    let dangling_expected = BootstrapBridge {
        matched_key: None,
        ..guarded_bridge()
    };
    if bootstrap_bridge_step(&dangling_expected, &sha).is_ok() {
        return Err(fail(
            "shadow guard accepted an expected key without its matched key",
        ));
    }
    for shadow in [String::new(), "has space".to_owned(), "x".repeat(65)] {
        let bridge = BootstrapBridge {
            shadow_state: Some(shadow),
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an untrusted shadow state"));
        }
    }
    for member in [
        "../escape".to_owned(),
        "/absolute".to_owned(),
        String::new(),
    ] {
        let bridge = BootstrapBridge {
            member,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an escaping member"));
        }
    }
    for staged in ["relative/staged".to_owned(), String::new()] {
        let bridge = BootstrapBridge {
            staged_path: staged,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an unowned staged path"));
        }
    }
    for max_bytes in [0, u64::MAX] {
        let bridge = BootstrapBridge {
            max_bytes,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an unbounded payload"));
        }
    }
    if bootstrap_bridge_step(&guarded_bridge(), "not-a-sha").is_ok() {
        return Err(fail("shadow guard accepted a foreign release digest"));
    }
    Ok(())
}

fn check_rejects_ledger_drift() -> Result<(), Box<dyn Error>> {
    let mut jobs = mode_jobs(None);
    if let Some(required) = jobs.get_mut(REQUIRED_JOB_ID) {
        required.needs = Vec::new();
    }
    if NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, &jobs).is_ok() {
        return Err(fail("ledger check accepted an empty required inventory"));
    }
    if NeedsConclusions::from_finalized_jobs("ghost-gate", &mode_jobs(None)).is_ok() {
        return Err(fail("ledger check accepted a missing required gate"));
    }
    let conclusions = match NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, &mode_jobs(None))
    {
        Ok(conclusions) => conclusions,
        Err(error) => return Err(fail(&format!("ledger check lost its baseline:{error}"))),
    };
    let mut drifted = mode_jobs(None);
    if let Some(required) = drifted.get_mut(REQUIRED_JOB_ID) {
        required.needs = vec!["reader".to_owned()];
    }
    if conclusions.gate_matches(&drifted) {
        return Err(fail("ledger check matched a drifted producer set"));
    }
    Ok(())
}

fn check_rejects_branch_collisions() -> Result<(), Box<dyn Error>> {
    let mut jobs = branch_jobs();
    let spec = PrepOverlap {
        download_job: "download".to_owned(),
        image_job: "download".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut jobs, &spec).is_ok() {
        return Err(fail("static branch accepted colliding ledgers"));
    }
    let spec = PrepOverlap {
        download_job: "ghost".to_owned(),
        image_job: "image".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut jobs, &spec).is_ok() {
        return Err(fail("static branch accepted an unknown ledger"));
    }
    let mut cyclic = branch_jobs();
    if let Some(download) = cyclic.get_mut("download") {
        download.needs = vec!["join".to_owned()];
    }
    if let Some(join) = cyclic.get_mut("join") {
        join.needs = vec!["download".to_owned()];
    }
    let spec = PrepOverlap {
        download_job: "download".to_owned(),
        image_job: "image".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut cyclic, &spec).is_ok() {
        return Err(fail("static branch accepted a ledger cycle"));
    }
    if RunsOn::parse("scale-set:velnor").is_ok() {
        return Err(fail("platform limit accepted a malformed scale-set token"));
    }
    Ok(())
}

#[test]
fn gc1_workflow_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_untrusted_event_writes()?;
    check_rejects_authority_mismatch()?;
    check_rejects_shadow_confusion()?;
    check_rejects_ledger_drift()?;
    check_rejects_branch_collisions()?;
    Ok(())
}
