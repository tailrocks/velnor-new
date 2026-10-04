//! Controller fixtures verify dispatch, cancellation, and terminal identity.

use std::error::Error;
use std::fs;
use std::io;

use super::Fixture;

#[test]
fn controller_dispatches_and_cancels_only_the_returned_validated_run() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new("controller-good", false)?;
    let (_, dispatch) = fixture.dispatch("good")?;
    assert!(dispatch.contains("dispatch_status=200\n"));
    assert!(dispatch.contains("workflow_id=77\n"));
    assert!(dispatch.contains("workflow_run_id=123\n"));
    assert!(
        dispatch.contains(
            "run_url=https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123\n"
        )
    );
    let ready = fixture.readiness("good")?;
    assert!(ready.contains("ready=true\n"), "{ready}");
    assert!(ready.contains("reason=exact_identity_and_cancel_window_ready\n"));
    let cancel = fixture.cancel("good")?;
    assert!(cancel.contains("cancel_requested=true\n"), "{cancel}");
    assert!(cancel.contains("cancel_status=202\n"));
    assert!(cancel.contains("post_revalidated=true\n"));
    let log = fixture.log()?;
    assert!(log.contains(
        "POST /repos/tailrocks/velnor-new/actions/workflows/qualification.yml/dispatches"
    ));
    assert!(log.contains("POST /repos/tailrocks/velnor-new/actions/runs/123/cancel\n"));
    assert!(log.lines().all(|line| line.starts_with("POST ")));
    let curl_log = fixture.curl_log()?;
    assert!(curl_log.contains("workflow-api authorized=true"));
    assert!(curl_log.contains("child-run-api authorized=true"));
    assert_eq!(
        curl_log
            .lines()
            .filter(|line| *line == "artifact-api authorized=true")
            .count(),
        2
    );
    assert_eq!(
        curl_log
            .lines()
            .filter(|line| *line == "artifact-signed authorized=false")
            .count(),
        2
    );
    assert!(!curl_log.contains("fixture-secret-token"));
    assert!(!curl_log.contains("sig=fixture-only"));
    assert!(
        !fixture
            .root
            .join("mbx-cancel-controller/artifact.curlrc")
            .exists()
    );
    assert!(
        !fixture
            .root
            .join("mbx-cancel-controller/artifact-response.headers")
            .exists()
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

#[test]
fn terminal_output_rejects_invalid_run_enums_before_writing() -> Result<(), Box<dyn Error>> {
    for mode in [
        "terminal-invalid-status",
        "terminal-unknown-status",
        "terminal-invalid-conclusion",
        "terminal-unknown-conclusion",
        "terminal-null-conclusion",
    ] {
        let fixture = Fixture::new(&format!("terminal-{mode}"), false)?;
        let output = fixture.wait_terminal(mode)?;
        assert_eq!(output, "terminal=false\nterminal_state=unknown\n", "{mode}");
        fs::remove_dir_all(fixture.root)?;
    }

    let fixture = Fixture::new("terminal-valid-cancelled", false)?;
    let output = fixture.wait_terminal("terminal-good")?;
    assert_eq!(
        output,
        "terminal=true\nterminal_state=completed/cancelled\n"
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

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
