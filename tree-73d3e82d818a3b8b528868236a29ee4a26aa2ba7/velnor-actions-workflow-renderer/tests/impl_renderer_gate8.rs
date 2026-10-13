//! Gate-8 renderer cases: artifacts, manifest script, rehead, release.
use velnor_actions_workflow_renderer::steps::{
    CANDIDATE_ARTIFACT_NAME, candidate_manifest_script, download_artifact_step,
    rehead_actionlint_marker, upload_artifact_step,
};

#[test]
fn artifact_steps_pin_actions_and_reject_empty() {
    let up = upload_artifact_step(CANDIDATE_ARTIFACT_NAME, "$RUNNER_TEMP/velnor/out")
        .map(|step| step.name);
    assert_eq!(up, Ok("Upload candidate".to_owned()));
    let down = download_artifact_step(CANDIDATE_ARTIFACT_NAME, "$RUNNER_TEMP/velnor/in")
        .map(|step| step.name);
    assert_eq!(down, Ok("Download candidate".to_owned()));
    assert!(upload_artifact_step("", "p").is_err());
    assert!(download_artifact_step("n", "").is_err());
}

#[test]
fn manifest_script_carries_contract_keys_without_substitution() {
    let script = candidate_manifest_script(
        "x86_64-unknown-linux-gnu",
        "rust@1.98.1+mr-boxington@1.19.0",
    );
    for key in ["commit", "target", "toolchain", "sha256", "GITHUB_SHA"] {
        assert!(script.contains(key), "missing {key}");
    }
    assert!(!script.contains("$(") && !script.contains('`'));
}

#[test]
fn rehead_swaps_first_line_only() {
    let out =
        rehead_actionlint_marker("# old header\nbody: 1\n", "0.1.0").map_err(|err| err.to_string());
    assert!(out.is_ok_and(|text| text.ends_with("body: 1\n") && !text.contains("old header")));
    assert!(rehead_actionlint_marker("no-newline", "0.1.0").is_err());
}
