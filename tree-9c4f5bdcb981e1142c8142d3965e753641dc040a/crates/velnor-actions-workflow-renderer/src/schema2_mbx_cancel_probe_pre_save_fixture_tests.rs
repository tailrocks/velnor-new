//! Execute the pre-save artifact-readiness path against the bounded API fixture.

use std::error::Error;
use std::fs;

use super::controller_fixtures::Fixture;

#[test]
fn pre_save_controller_validates_artifact_and_cancels_exact_live_wait_step()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("pre-save-controller", false)?;
    let (_, dispatch) = fixture.dispatch("good")?;
    assert!(dispatch.contains("workflow_run_id=123\n"), "{dispatch}");
    let ready = fixture.readiness("good")?;
    assert!(ready.contains("ready=true\n"), "{ready}");
    let cancel = fixture.cancel("good")?;
    assert!(cancel.contains("cancel_requested=true\n"), "{cancel}");
    let cache = fs::read_to_string(
        fixture
            .root
            .join("mbx-cancel-controller/cache-before-exact.json"),
    )?;
    assert!(cache.contains("\"count\":0"), "{cache}");
    let log = fixture.log()?;
    assert!(log.contains("/actions/runs/123/artifacts?per_page=100"));
    assert!(log.contains("POST /repos/tailrocks/velnor-new/actions/runs/123/cancel\n"));
    let transport = fs::read_to_string(fixture.root.join("curl.log"))?;
    assert_eq!(transport.lines().count(), 4, "{transport}");
    assert!(!transport.contains("fixture-secret-token"));
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}
