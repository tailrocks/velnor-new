//! Controller transfer and immediate revalidation reject-path fixtures.

use std::error::Error;
use std::fs;

use super::controller_fixtures::Fixture;

#[test]
fn artifact_redirects_are_https_bounded_and_tokenless() -> Result<(), Box<dyn Error>> {
    for mode in [
        "artifact-no-location",
        "artifact-http-location",
        "artifact-duplicate-location",
        "artifact-large-header",
        "artifact-large-signed-header",
        "artifact-signed-redirect",
        "artifact-signed-http-redirect",
        "artifact-large-body",
    ] {
        assert_artifact_rejected(mode)?;
    }
    Ok(())
}

fn assert_artifact_rejected(mode: &str) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(mode, false)?;
    fixture.dispatch(mode)?;
    let readiness = fixture.readiness(mode)?;
    assert!(readiness.contains("ready=false"), "{mode}: {readiness}");
    assert!(
        readiness.contains("reason=victim_receipt_invalid"),
        "{mode}: {readiness}"
    );
    let cancel = fixture.cancel(mode)?;
    assert!(
        cancel.contains("cancel_requested=false"),
        "{mode}: {cancel}"
    );
    assert!(
        !fixture.log()?.contains("/actions/runs/123/cancel"),
        "{mode}"
    );
    let curl_log = fs::read_to_string(fixture.root.join("curl.log"))?;
    assert!(
        !curl_log.contains("fixture-only"),
        "{mode}: leaked signed URL"
    );
    assert!(
        !curl_log.contains("fixture-secret-token"),
        "{mode}: leaked token"
    );
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
fn exact_child_binding_is_revalidated_before_cancel() -> Result<(), Box<dyn Error>> {
    for mode in [
        "revalidate-actor",
        "revalidate-attempt",
        "revalidate-job",
        "revalidate-artifact",
    ] {
        let fixture = Fixture::new(mode, false)?;
        fixture.dispatch(mode)?;
        let readiness = fixture.readiness(mode)?;
        assert!(readiness.contains("ready=true"), "{mode}: {readiness}");
        let cancel = fixture.cancel(mode)?;
        assert!(
            cancel.contains("cancel_requested=false"),
            "{mode}: {cancel}"
        );
        assert!(
            !fixture.log()?.contains("/actions/runs/123/cancel"),
            "{mode} reached cancellation endpoint"
        );
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}
