use velnor_runner_github::parse_inner_messages;

const JOB: &str = "67c5c946-116b-52aa-9566-1d8de67a2a67";

fn id(scale_set: Option<&str>, requested: &str) -> Option<i64> {
    let mut msg = serde_json::json!({
        "messageType": "JobAssigned",
        "runnerRequestId": 0,
        "jobId": JOB,
        "requestAssignedTime": requested,
    });
    if let Some(time) = scale_set {
        msg["scaleSetAssignTime"] = time.into();
    }
    let jobs = parse_inner_messages(&format!("[{msg}]")).ok()?;
    jobs.first()?.request_id
}

#[test]
fn zero_instant_spellings_are_ignored() {
    let real = "2026-11-06T22:55:21.7495747Z";
    let expected = id(None, real);
    assert!(expected.is_some());
    for zero in [
        "0001-01-01T00:00:00Z",
        "0001-01-01T00:00:00",
        "0001-01-01T00:00:00.0000000Z",
        "0001-01-01T00:00:00+00:00",
    ] {
        assert_eq!(id(Some(zero), real), expected, "{zero}");
    }
    assert_ne!(
        id(Some("0001-01-01T00:00:00"), "2026-11-06T23:00:00Z"),
        expected
    );
}
