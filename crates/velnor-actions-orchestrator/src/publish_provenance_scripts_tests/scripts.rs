use std::fs;

use super::{
    CosignPin, DIR, SUBJECT, TestResult, fixture, hex_sha256, log, run, sign, tools_present,
};
use velnor_actions_workflow_renderer::publish_scripts::provenance::{sign_script, verify_script};

#[test]
fn generated_bundle_scripts_keep_substitution_out_and_remove_owned_scratch() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let pin = hex_sha256(&fx.cosign)?;
    let signing = sign_script(
        "gh() { command gh \"$@\"; }",
        DIR,
        SUBJECT,
        CosignPin {
            version: "3.1.4",
            sha256: &pin,
            size: usize::try_from(fs::metadata(&fx.cosign)?.len())?,
        },
    )?;
    let verifying = verify_script("gh() { command gh \"$@\"; }", DIR, SUBJECT);
    assert!(!signing.contains("$("), "{signing}");
    assert!(
        signing.contains("wc -c < \"$w/cosign\" > \"$w/cosign.bytes\"")
            && signing.contains("case \"$actual_size\" in ''|*[!0-9]*)"),
        "{signing}"
    );
    assert!(!verifying.contains("$("), "{verifying}");
    assert!(
        verifying.contains("w=\"$RUNNER_TEMP/velnor-verify\"; mkdir -m 700 -- \"$w\"")
            && verifying.contains("trap 'rm -rf -- \"$w\"' EXIT"),
        "{verifying}"
    );

    assert!(sign(&fx, &pin, &[])?.status.success());
    let ok = run(&fx, &verifying, &[])?;
    assert!(ok.status.success(), "{ok:?}");
    assert!(
        !fx.temp.join("velnor-verify").exists(),
        "verification scratch is removed on success"
    );

    let refused = run(&fx, &verifying, &[("FAKE_VERIFY", "1")])?;
    assert!(!refused.status.success(), "failed verifier is rejected");
    assert!(
        !fx.temp.join("velnor-verify").exists(),
        "verification scratch is removed after verifier failure"
    );
    Ok(())
}

#[test]
fn verification_refuses_an_existing_scratch_directory_without_removing_it() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let scratch = fx.temp.join("velnor-verify");
    fs::create_dir(&scratch)?;
    let sentinel = scratch.join("preexisting");
    fs::write(&sentinel, "owned elsewhere")?;

    let verifying = verify_script("gh() { command gh \"$@\"; }", DIR, SUBJECT);
    let output = run(&fx, &verifying, &[])?;
    assert!(
        !output.status.success(),
        "existing scratch must fail closed"
    );
    assert_eq!(fs::read(&sentinel)?, b"owned elsewhere");
    assert!(log(&fx, "gh").is_empty(), "verification did not start");
    Ok(())
}
