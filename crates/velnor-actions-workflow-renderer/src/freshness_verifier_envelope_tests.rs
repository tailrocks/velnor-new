use super::spec;

const EXPECTED_CREDENTIAL_PRELUDE: &str = "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR;";

fn assert_freshness_verifier_security(verifier: &str) {
    assert!(
        verifier.contains(&format!("shell: {}", crate::VERIFY_VERIFIER_SHELL)),
        "freshness uses the trusted verifier shell:\n{verifier}"
    );
    assert!(
        verifier.contains(&format!("PATH: {}", crate::VERIFY_PATH)),
        "freshness resolves helpers only from the fixed verifier path:\n{verifier}"
    );
    assert!(
        verifier.contains("function_env_scan_complete"),
        "the canonical verifier wrapper keeps its startup guard and reviewed payload:\n{verifier}"
    );
    let prelude_position = verifier
        .find(EXPECTED_CREDENTIAL_PRELUDE)
        .expect("complete fixed credential-unset prelude");
    let payload_position = verifier
        .find("echo verify-rust")
        .expect("freshness payload follows the prelude");
    assert!(
        prelude_position < payload_position,
        "all credentials are unset before the reviewed payload:\n{verifier}"
    );
}

#[test]
fn freshness_verifier_keeps_the_startup_safe_credential_envelope() {
    let rendered = crate::freshness::render_freshness_workflow(&spec()).expect("freshness render");
    let start = rendered
        .bytes
        .find("      - name: Verify Rust channel manifest")
        .expect("installed Rust verifier step");
    let verifier = &rendered.bytes[start..];
    let end = verifier
        .find("\n      - name: Check upstream freshness")
        .expect("probe follows verifier");
    let verifier = &verifier[..end];

    assert_freshness_verifier_security(verifier);
    assert!(verifier.contains("-u BASH_ENV -u ENV"), "{verifier}");
}
