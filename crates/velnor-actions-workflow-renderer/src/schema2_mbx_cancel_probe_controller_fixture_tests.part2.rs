#[test]
fn malformed_dispatch_or_victim_identity_never_reaches_cancel() -> Result<(), Box<dyn Error>> {
    for mode in ["missing-id", "bad-url", "initial-mismatch"] {
        let fixture = Fixture::new(&format!("controller-{mode}"), false)?;
        let (_, dispatch) = fixture.dispatch(mode)?;
        assert!(
            dispatch.contains("dispatch_status=200\n"),
            "{mode}: {dispatch}"
        );
        let run_id = dispatch
            .lines()
            .find_map(|line| line.strip_prefix("workflow_run_id="))
            .ok_or_else(|| io::Error::other("dispatch run ID output missing"))?;
        assert!(run_id.is_empty(), "{mode}: {dispatch}");
        let ready = fixture.readiness_with_run(mode, run_id)?;
        assert!(ready.contains("ready=false\n"), "{mode}: {ready}");
        let cancel = fixture.cancel_with_run(mode, run_id)?;
        assert!(
            cancel.contains("cancel_requested=false\n"),
            "{mode}: {cancel}"
        );
        assert!(!fixture.log()?.contains("/actions/runs/123/cancel"));
        let run_queries = fixture
            .curl_log()?
            .lines()
            .filter(|line| *line == "child-run-api authorized=true")
            .count();
        assert_eq!(
            run_queries,
            usize::from(mode == "initial-mismatch"),
            "{mode}"
        );
        fs::remove_dir_all(fixture.root)?;
    }

    let mismatch = Fixture::new("controller-run-mismatch", false)?;
    mismatch.dispatch("mismatch")?;
    let ready = mismatch.readiness("mismatch")?;
    assert!(ready.contains("ready=false\n"));
    assert!(ready.contains("reason=exact_run_identity_mismatch\n"));
    let cancel = mismatch.cancel("mismatch")?;
    assert!(cancel.contains("cancel_requested=false\n"));
    assert!(!mismatch.log()?.contains("/actions/runs/123/cancel"));

    let bad_artifact = Fixture::new("controller-bad-digest", true)?;
    bad_artifact.dispatch("good")?;
    let ready = bad_artifact.readiness("good")?;
    assert!(ready.contains("ready=false\n"));
    assert!(ready.contains("reason=victim_receipt_invalid\n"));
    let cancel = bad_artifact.cancel("good")?;
    assert!(cancel.contains("cancel_requested=false\n"));
    assert!(!bad_artifact.log()?.contains("/actions/runs/123/cancel"));

    fs::remove_dir_all(mismatch.root)?;
    fs::remove_dir_all(bad_artifact.root)?;
    Ok(())
}
