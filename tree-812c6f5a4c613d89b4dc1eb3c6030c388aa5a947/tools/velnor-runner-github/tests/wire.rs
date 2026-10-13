//! Sanitized fixtures for the pinned `actions/scaleset` message shape.

use velnor_runner_github::{
    AcquireOutcome, CAPACITY_HEADER, Certainty, EncodedJit, InnerKind, Label, Poll, RefreshGate,
    ScaleSetView, StatusClass, TransportFail, WireError, accept_scale_set, acquire_path,
    capacity_header_value, classify_acquire, classify_status, create_body, effect_certainty,
    jit_path, last_message_query, may_ack, parse_poll,
};

const NULL_STATS: &str =
    r#"{"messageId":0,"messageType":"RunnerScaleSetJobMessages","body":"[]","statistics":null}"#;
const OMITTED_STATS: &str =
    r#"{"messageId":0,"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#;

#[test]
fn null_and_omitted_statistics_match_and_message_zero_is_real() -> Result<(), &'static str> {
    let null = parse_poll(200, NULL_STATS).map_err(|_| "null")?;
    let omitted = parse_poll(200, OMITTED_STATS).map_err(|_| "omitted")?;
    let Poll::Batch(left) = null else {
        return Err("batch");
    };
    let Poll::Batch(right) = omitted else {
        return Err("batch");
    };
    assert_eq!(left.statistics, None);
    assert_eq!(right.statistics, None);
    assert_eq!(left.message_id, 0);
    assert!(may_ack(&left, true));
    assert!(!may_ack(&left, false));
    Ok(())
}

#[test]
fn empty_poll_is_not_an_ack_and_unknown_kind_is_visible() -> Result<(), &'static str> {
    assert_eq!(parse_poll(202, ""), Ok(Poll::Empty));
    let body = r#"{"messageId":4,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobExploded\",\"runnerRequestId\":9},{\"messageType\":\"JobAvailable\",\"runnerRequestId\":3}]"}"#;
    let Poll::Batch(batch) = parse_poll(200, body).map_err(|_| "batch")? else {
        return Err("batch");
    };
    assert!(matches!(batch.jobs[0].kind, InnerKind::Unsupported(_)));
    assert_eq!(batch.jobs[1].request_id, Some(3));
    assert!(!may_ack(&batch, true));
    assert!(!may_ack(
        &velnor_runner_github::ParsedBatch {
            message_id: -1,
            statistics: None,
            jobs: Vec::new()
        },
        true
    ));
    Ok(())
}

#[test]
fn assigned_population_is_not_the_batch_length() -> Result<(), &'static str> {
    let raw = r#"{"messageId":1,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":1,\"jobId\":\"111\",\"requestLabels\":[\"velnor\",\"ubuntu-26.04-scale-set\"]}]","statistics":{"totalAvailableJobs":1,"totalAcquiredJobs":0,"totalAssignedJobs":5,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;
    let Poll::Batch(batch) = parse_poll(200, raw).map_err(|_| "batch")? else {
        return Err("batch");
    };
    let stats = batch.statistics.ok_or("stats")?;
    let len = i64::try_from(batch.jobs.len()).map_err(|_| "len")?;
    assert_eq!(stats.assigned_population(), 5);
    assert_ne!(stats.assigned_population(), len);
    assert_eq!(batch.jobs[0].job_id.as_deref(), Some("111"));
    assert_eq!(
        batch.jobs[0].labels,
        ["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()]
    );
    Ok(())
}

#[test]
fn completed_message_decodes_runner_identity_and_keeps_job_fields() -> Result<(), &'static str> {
    let raw = r#"{"messageId":2,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobCompleted\",\"runnerRequestId\":19,\"jobId\":\"111\",\"requestLabels\":[\"velnor\"],\"runnerId\":31,\"runnerName\":\"runner-31\",\"result\":\"succeeded\"}]"}"#;
    let Poll::Batch(batch) = parse_poll(200, raw).map_err(|_| "batch")? else {
        return Err("batch");
    };
    let job = batch.jobs.first().ok_or("job")?;
    assert_eq!(job.kind, InnerKind::Completed);
    assert_eq!(job.request_id, Some(19));
    assert_eq!(job.job_id.as_deref(), Some("111"));
    assert_eq!(job.labels, ["velnor"]);
    assert_eq!(job.runner_id, Some(31));
    assert_eq!(job.runner_name.as_deref(), Some("runner-31"));
    assert_eq!(job.result.as_deref(), Some("succeeded"));
    Ok(())
}

#[test]
fn scale_set_identity_and_opaque_job_id_are_preserved() -> Result<(), &'static str> {
    let raw = r#"{"messageId":3,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":19,\"jobId\":\"gha:job/119?attempt=2\",\"workflowRunId\":88,\"ownerName\":\"ChainArgos\",\"repositoryName\":\"java-monorepo\",\"eventName\":\"pull_request\",\"requestLabels\":[\"ubuntu-26.04-scale-set\"]}]"}"#;
    let Poll::Batch(batch) = parse_poll(200, raw).map_err(|_| "batch")? else {
        return Err("batch");
    };
    let job = batch.jobs.first().ok_or("job")?;
    assert_eq!(job.job_id.as_deref(), Some("gha:job/119?attempt=2"));
    assert_eq!(job.workflow_run_id, Some(88));
    assert_eq!(job.owner_name.as_deref(), Some("ChainArgos"));
    assert_eq!(job.repository_name.as_deref(), Some("java-monorepo"));
    assert_eq!(job.event_name.as_deref(), Some("pull_request"));
    Ok(())
}

#[test]
fn partial_acquire_outside_ids_and_noop() -> Result<(), &'static str> {
    let partial = classify_acquire(&[1, 2, 3], &[1, 3], &[]).map_err(|_| "partial")?;
    assert_eq!(partial, AcquireOutcome::Acquired(vec![1, 3]));
    assert_eq!(
        classify_acquire(&[1], &[9], &[]),
        Err(WireError::OutsideRequest)
    );
    assert_eq!(
        classify_acquire(&[1, 2], &[1, 2], &[2, 1]),
        Ok(AcquireOutcome::Noop)
    );
    assert_eq!(
        effect_certainty(TransportFail::Timeout),
        Certainty::Uncertain
    );
    assert_eq!(effect_certainty(TransportFail::Reset), Certainty::Uncertain);
    assert_eq!(
        effect_certainty(TransportFail::Http(403)),
        Certainty::Definite
    );
    assert_eq!(
        effect_certainty(TransportFail::Http(500)),
        Certainty::Uncertain
    );
    Ok(())
}

#[test]
fn refresh_is_single_flight_and_forbidden_is_terminal() -> Result<(), &'static str> {
    let gate = RefreshGate::new();
    assert_eq!(
        classify_status(401, &gate).map_err(|_| "once")?,
        StatusClass::RefreshOnce
    );
    assert_eq!(
        classify_status(401, &gate),
        Err(WireError::RefreshExhausted)
    );
    assert_eq!(classify_status(403, &gate), Err(WireError::Forbidden));
    assert_eq!(gate.started().map_err(|_| "count")?, 1);
    assert_eq!(
        classify_status(409, &gate).map_err(|_| "conflict")?,
        StatusClass::SessionConflict
    );
    Ok(())
}

#[test]
fn registration_disables_updates_and_rejects_hosted_label() -> Result<(), &'static str> {
    let body = create_body("ubuntu-26.04-scale-set").map_err(|_| "body")?;
    assert!(body.contains("\"RunnerSetting\""));
    assert!(body.contains("\"disableUpdate\":true"));
    assert!(!jit_path(7).contains("generate-jitconfig"));
    assert!(jit_path(7).ends_with("/7/generatejitconfig"));
    assert!(acquire_path(7).ends_with("/acquirejobs"));
    assert_eq!(last_message_query(0), None);
    assert_eq!(last_message_query(4).as_deref(), Some("lastMessageId=4"));
    assert_eq!(CAPACITY_HEADER, "X-ScaleSetMaxCapacity");
    assert_eq!(capacity_header_value(2), "2");
    let mut view: ScaleSetView = serde_json::from_str(
        r#"{"id":7,"name":"ubuntu-26.04-scale-set","labels":[{"name":"velnor"},{"name":"ubuntu-26.04-scale-set"}],"RunnerSetting":{"disableUpdate":true}}"#,
    )
    .map_err(|_| "view")?;
    accept_scale_set(&view, "ubuntu-26.04-scale-set").map_err(|_| "ok")?;
    view.labels.push(Label {
        name: "ubuntu-26.04".to_owned(),
        label_type: String::new(),
    });
    assert_eq!(
        accept_scale_set(&view, "ubuntu-26.04-scale-set"),
        Err(WireError::RegistrationRejected)
    );
    view.labels.pop();
    view.runner_setting.disable_update = false;
    assert_eq!(
        accept_scale_set(&view, "ubuntu-26.04-scale-set"),
        Err(WireError::RegistrationRejected)
    );
    Ok(())
}

#[test]
fn jit_debug_does_not_contain_the_secret() {
    let secret = "canary-jit-value";
    let jit = EncodedJit::new(secret.to_owned());
    let rendered = format!("{jit:?}");
    assert!(!rendered.contains(secret));
    assert_eq!(jit.expose(), secret);
}
