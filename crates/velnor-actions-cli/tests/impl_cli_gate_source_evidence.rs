//! Private gate coverage for source-evidence collection.

use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

fn valid_context<'a>(temp: &'a str, event_path: &'a str) -> Vec<(&'static str, &'a str)> {
    vec![
        (
            "VELNOR_INTERNAL_OP",
            "write-qualification-source-evidence-v1",
        ),
        ("RUNNER_TEMP", temp),
        ("GITHUB_RUN_ID", "700"),
        ("GITHUB_RUN_ATTEMPT", "1"),
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_EVENT_PATH", event_path),
        ("GITHUB_REF_PROTECTED", "true"),
        ("GITHUB_WORKSPACE", temp),
        ("GITHUB_JOB", "qualification-rust-demo"),
        ("GITHUB_REPOSITORY", "owner/repo"),
        ("GITHUB_REF", "refs/heads/main"),
        (
            "GITHUB_WORKFLOW_REF",
            "owner/repo/.github/workflows/ci.yml@refs/heads/main",
        ),
        (
            "GITHUB_WORKFLOW_SHA",
            "0123456789abcdef0123456789abcdef01234567",
        ),
        ("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567"),
    ]
}

#[test]
fn source_evidence_requires_runner_coordinates_and_authenticated_event_context()
-> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-source-evidence")?;
    let request = temp.join("untrusted-request.json");
    std::fs::write(&request, "not source evidence input")?;
    let bare = spawn_isolated(&[], &[], &temp)?;
    let temp_text = temp.to_str().ok_or("temp path is not UTF-8")?;
    let event_path = temp.join("event.json");
    std::fs::write(&event_path, "{}")?;
    let event_text = event_path.to_str().ok_or("event path is not UTF-8")?;
    let request_text = request.to_str().ok_or("request path is not UTF-8")?;

    for missing in [
        "RUNNER_TEMP",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_EVENT_PATH",
        "GITHUB_REF_PROTECTED",
        "GITHUB_WORKSPACE",
    ] {
        let mut vars = valid_context(temp_text, event_text);
        vars.retain(|(name, _)| *name != missing);
        vars.push(("VELNOR_REQUEST_FILE", request_text));
        let gated = spawn_isolated(&[], &vars, &temp)?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }

    let mut wrong_event = valid_context(temp_text, event_text);
    wrong_event
        .iter_mut()
        .find(|(name, _)| *name == "GITHUB_EVENT_NAME")
        .expect("event name")
        .1 = "push";
    wrong_event.push(("VELNOR_REQUEST_FILE", request_text));
    let gated = spawn_isolated(&[], &wrong_event, &temp)?;
    assert_eq!(code(&gated), 2);
    assert_identical(&bare, &gated);

    let mut entered = valid_context(temp_text, event_text);
    entered.push(("VELNOR_REQUEST_FILE", request_text));
    let output = spawn_isolated(&[], &entered, &temp)?;
    assert_eq!(code(&output), 1);
    assert_eq!(output.stdout, [] as [u8; 0]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("internal request failed"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(request_text));
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("write-qualification-source-evidence-v1")
    );
    cleanup(&temp);
    Ok(())
}
