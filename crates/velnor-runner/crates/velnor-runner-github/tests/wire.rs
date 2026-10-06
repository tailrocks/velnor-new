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
    assert_eq!(
        parse_poll(
            200,
            r#"{"messageType":"RunnerScaleSetJobMessages","body":"[]"}"#
        ),
        Err(WireError::Malformed)
    );
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
    assert_eq!(
        batch.raw_body,
        r#"[{"messageType":"JobExploded","runnerRequestId":9},{"messageType":"JobAvailable","runnerRequestId":3}]"#
    );
    assert_eq!(
        velnor_runner_github::parse_inner_messages(&batch.raw_body),
        Ok(batch.jobs.clone())
    );
    assert!(!may_ack(&batch, true));
    assert!(!may_ack(
        &velnor_runner_github::ParsedBatch {
            message_id: -1,
            raw_body: String::new(),
            statistics: None,
            jobs: Vec::new()
        },
        true
    ));
    Ok(())
}

#[test]
fn started_and_completed_fields_match_their_pinned_message_types() -> Result<(), &'static str> {
    let raw = r#"{"messageId":8,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobStarted\",\"runnerRequestId\":17,\"runnerId\":31,\"runnerName\":\"runner-31\"},{\"messageType\":\"JobCompleted\",\"runnerRequestId\":18,\"runnerId\":32,\"runnerName\":\"runner-32\",\"result\":\"Succeeded\"},{\"messageType\":\"JobAvailable\",\"runnerRequestId\":19,\"runnerId\":99,\"runnerName\":\"ignored\",\"result\":\"ignored\"}]"}"#;
    let Poll::Batch(batch) = parse_poll(200, raw).map_err(|_| "batch")? else {
        return Err("batch");
    };
    let started = batch.jobs.first().ok_or("started")?;
    assert_eq!(started.kind, InnerKind::Started);
    assert_eq!(started.request_id, Some(17));
    assert_eq!(started.runner_id, Some(31));
    assert_eq!(started.runner_name.as_deref(), Some("runner-31"));
    assert_eq!(started.result, None);
    let completed = batch.jobs.get(1).ok_or("completed")?;
    assert_eq!(completed.kind, InnerKind::Completed);
    assert_eq!(completed.request_id, Some(18));
    assert_eq!(completed.runner_id, Some(32));
    assert_eq!(completed.runner_name.as_deref(), Some("runner-32"));
    assert_eq!(completed.result.as_deref(), Some("Succeeded"));
    let available = batch.jobs.get(2).ok_or("available")?;
    assert_eq!(available.request_id, Some(19));
    assert_eq!(available.runner_id, None);
    assert_eq!(available.runner_name, None);
    assert_eq!(available.result, None);
    Ok(())
}

#[test]
fn malformed_runner_fields_quarantine_the_poll_batch() {
    let raw = r#"{"messageId":8,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobCompleted\",\"runnerRequestId\":18,\"runnerId\":\"32\",\"runnerName\":\"runner-32\",\"result\":\"Succeeded\"}]"}"#;
    let Poll::Quarantined(quarantined) = parse_poll(200, raw).expect("quarantined") else {
        panic!("expected quarantine, got {:?}", parse_poll(200, raw));
    };
    assert_eq!(quarantined.message_id, 8);
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
fn poll_body_enforces_protocol_byte_and_message_limits() -> Result<(), &'static str> {
    let accepted = poll_with_message_count(velnor_runner_github::MAX_POLL_MESSAGES);
    let Poll::Batch(batch) = parse_poll(200, &accepted).map_err(|_| "accepted limit")? else {
        return Err("accepted batch");
    };
    assert_eq!(batch.jobs.len(), velnor_runner_github::MAX_POLL_MESSAGES);

    let rejected = poll_with_message_count(velnor_runner_github::MAX_POLL_MESSAGES + 1);
    assert!(matches!(
        parse_poll(200, &rejected),
        Ok(Poll::Quarantined(_))
    ));
    let at_byte_limit = format!(
        "[{}]",
        " ".repeat(velnor_runner_github::MAX_POLL_BODY_BYTES - 2)
    );
    assert_eq!(
        velnor_runner_github::parse_inner_messages(&at_byte_limit),
        Ok(Vec::new())
    );
    assert_eq!(
        velnor_runner_github::parse_inner_messages(&format!(
            "[{}]",
            " ".repeat(velnor_runner_github::MAX_POLL_BODY_BYTES - 1)
        )),
        Err(WireError::Malformed)
    );
    Ok(())
}

#[test]
fn outer_message_id_is_authoritative_over_inner_fields() -> Result<(), &'static str> {
    let raw = r#"{"messageId":17,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":9,\"messageId\":999}]"}"#;
    let Poll::Batch(batch) = parse_poll(200, raw).map_err(|_| "poll")? else {
        return Err("batch");
    };
    assert_eq!(batch.message_id, 17);
    assert_eq!(batch.jobs[0].request_id, Some(9));
    assert!(batch.jobs[0].fields.contains(&"messageId".to_owned()));
    Ok(())
}

#[test]
fn malformed_bounded_inner_body_keeps_outer_id_and_exact_body() {
    let raw = r#"{"messageId":18,"messageType":"RunnerScaleSetJobMessages","body":"[{bad json]"}"#;
    assert_eq!(
        parse_poll(200, raw),
        Ok(Poll::Quarantined(velnor_runner_github::QuarantinedBatch {
            message_id: 18,
            raw_body: "[{bad json]".to_owned(),
        }))
    );

    let missing_id = r#"{"messageType":"RunnerScaleSetJobMessages","body":"[{bad json]"}"#;
    assert_eq!(parse_poll(200, missing_id), Err(WireError::Malformed));
    let negative_id = r#"{"messageId":-1,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobCompleted\",\"runnerRequestId\":9,\"runnerId\":17,\"runnerName\":\"runner-9\"}]"}"#;
    assert_eq!(parse_poll(200, negative_id), Err(WireError::Malformed));
    let oversized = format!(
        r#"{{"messageId":19,"messageType":"RunnerScaleSetJobMessages","body":"{}"}}"#,
        "x".repeat(velnor_runner_github::MAX_POLL_BODY_BYTES + 1)
    );
    assert_eq!(parse_poll(200, &oversized), Err(WireError::Malformed));
}

fn poll_with_message_count(count: usize) -> String {
    let item = r#"{"messageType":"JobAvailable"}"#;
    let body = std::iter::repeat_n(item, count)
        .collect::<Vec<_>>()
        .join(",");
    let escaped = body.replace('"', "\\\"");
    format!(r#"{{"messageId":1,"messageType":"RunnerScaleSetJobMessages","body":"[{escaped}]"}}"#)
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
