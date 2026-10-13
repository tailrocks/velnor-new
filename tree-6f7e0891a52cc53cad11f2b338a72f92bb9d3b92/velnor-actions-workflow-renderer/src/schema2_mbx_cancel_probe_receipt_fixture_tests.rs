//! The observer accepts only the exact controller receipt and child run.

use std::error::Error;
use std::fs;

use super::controller_fixtures::{Fixture, expected_key};
use super::{prepare_observer_root, run_bash};
use crate::schema2::mbx_cancel_probe::scripts;

const CONTROLLER_MODE: &str = "mbx-cancel-during-save-controller";
#[test]
fn validator_accepts_exact_dotted_key_and_bound_child() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("valid-controller-receipt", false)?;
    let output = execute_validator(
        &fixture,
        &receipt(&expected_key()),
        &format!(r#"{{"inputs":{{"mode":"{CONTROLLER_MODE}","probe_id":""}}}}"#),
    )?;
    assert!(output.contains("should_observe=true\n"), "{output}");
    assert!(output.contains("child_run_id=123\n"), "{output}");
    assert!(
        output.contains("generation=velnor-mbx-1.22.0\n"),
        "{output}"
    );
    assert!(
        output.contains(&format!("cache_key={}\n", expected_key())),
        "{output}"
    );
    fs::remove_dir_all(fixture.root)?;
    Ok(())
}

#[test]
fn malformed_controller_receipt_never_observes_child() -> Result<(), Box<dyn Error>> {
    let mut cases = vec![receipt("bad/key")];
    cases.push(receipt(&expected_key()).replace(
        "\"generation\":\"velnor-mbx-1.22.0\"",
        "\"generation\":\"velnor-mbx-1.22.0\\nshould_observe=true\"",
    ));
    cases.push(receipt(&expected_key()).replace(
        "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6",
        "jdx/mr-boxington-action@attacker",
    ));
    for (index, invalid) in cases.into_iter().enumerate() {
        let fixture = Fixture::new(&format!("malformed-controller-receipt-{index}"), false)?;
        let output = execute_validator(
            &fixture,
            &invalid,
            &format!(r#"{{"inputs":{{"mode":"{CONTROLLER_MODE}","probe_id":""}}}}"#),
        )?;
        assert!(output.contains("should_observe=false\n"), "{output}");
        if fixture.root.join("gh.log").exists() {
            assert!(!fixture.log()?.contains("actions/runs/"));
        }
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}

#[test]
fn missing_or_nonstring_controller_probe_id_never_observes_child() -> Result<(), Box<dyn Error>> {
    for (index, event) in [
        format!(r#"{{"inputs":{{"mode":"{CONTROLLER_MODE}"}}}}"#),
        format!(r#"{{"inputs":{{"mode":"{CONTROLLER_MODE}","probe_id":null}}}}"#),
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("invalid-controller-event-{index}"), false)?;
        let output = execute_validator(&fixture, &receipt(&expected_key()), &event)?;
        assert!(output.contains("should_observe=false\n"), "{output}");
        if fixture.root.join("gh.log").exists() {
            assert!(!fixture.log()?.contains("actions/runs/"));
        }
        fs::remove_dir_all(fixture.root)?;
    }
    Ok(())
}

fn execute_validator(
    fixture: &Fixture,
    receipt: &str,
    event_payload: &str,
) -> Result<String, Box<dyn Error>> {
    let output = fixture.output("receipt-validation");
    fs::write(&output, "")?;
    let env = fixture.env(&output, "receipt-valid");
    prepare_observer_root(&fixture.root, &fixture.bin, &env)?;
    let path = fixture
        .root
        .join("mbx-cancel-observer/controller-receipt/receipt.json");
    fs::write(&path, receipt)?;
    let event = fixture.root.join("event.json");
    fs::write(&event, event_payload)?;
    fs::write(&output, "")?;
    let mut env = fixture.env(&output, "receipt-valid");
    env.push(("GITHUB_EVENT_PATH".to_owned(), event.display().to_string()));
    let result = run_bash(
        scripts::VALIDATE_CONTROLLER_RECEIPT,
        &fixture.root,
        &fixture.bin,
        &env,
    )?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(fs::read_to_string(output)?)
}

fn receipt(cache_key: &str) -> String {
    const TEMPLATE: &str = r#"{"schema":1,"probe_id":"0123456789abcdef0123456789abcdef","phase":"during-save","controller_mode":"mbx-cancel-during-save-controller","repository":"tailrocks/velnor-new","controller_run_id":"900","controller_attempt":"1","controller_source_sha":"cccccccccccccccccccccccccccccccccccccccc","controller_actor":"fixture-controller","child_run_id":"123","child_workflow_id":"77","child_run_url":"https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123","dispatch_status":"200","ready":true,"ready_reason":"exact_identity_and_cancel_window_ready","cancel_requested":true,"cancel_status":"202","cancel_reason":"exact_child_cancel_accepted","post_revalidated":true,"cancel_request_started_at":"2026-10-04T00:00:10Z","terminal":true,"terminal_state":"completed/cancelled","cache_before":{"count":0,"caches":[]},"victim":{"probe_id":"0123456789abcdef0123456789abcdef","child_run_id":"123","child_attempt":"1","source_sha":"cccccccccccccccccccccccccccccccccccccccc","cache_key":"CACHE_KEY_PLACEHOLDER","cache_scope":"qualification-mbx-v1/cancel-during-save-victim","generation":"velnor-mbx-1.22.0","rustc_identity":"13936cde15db9d31620cb9989927cdfa06948615fc6b8291d7fba92f191a18ec","mbx_version":"1.22.0","mode":"mbx-cancel-during-save-victim","phase":"during-save","actor":"github-actions[bot]","repository":"tailrocks/velnor-new","workflow_path":".github/workflows/qualification.yml","event":"workflow_dispatch","ref":"refs/heads/main","mbx_action_uses":"jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6","mbx_resolved_version":"1.22.0","rust_version":"1.98.1","mise_action_uses":"jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5","mise_version":"2025.9.5","mise_sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}}"#;
    TEMPLATE.replace("CACHE_KEY_PLACEHOLDER", cache_key)
}
