//! Release-target and candidate-manifest cases.
use velnor_actions_contract::{
    CandidateArtifactManifest, ContractError, RELEASE_MANIFEST_FILENAME, ReleaseManifest,
    SUPPORTED_TARGETS, asset_filename, is_supported_target, target_for_runner_label,
};

#[test]
fn supported_targets_and_naming() {
    assert_eq!(SUPPORTED_TARGETS.len(), 3);
    assert!(is_supported_target("x86_64-unknown-linux-gnu"));
    assert!(!is_supported_target("wasm32-unknown-unknown"));
    assert_eq!(
        RELEASE_MANIFEST_FILENAME,
        "velnor-actions-release-manifest.json"
    );
    assert_eq!(
        asset_filename("0.1.0", "x86_64-unknown-linux-gnu"),
        "velnor-actions-0.1.0-x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        target_for_runner_label("ubuntu-26.04"),
        Some(SUPPORTED_TARGETS[0])
    );
    assert!(target_for_runner_label("ubuntu-26.04-arm").is_none());
}

#[test]
fn release_manifest_json_round_trip_and_tamper() -> Result<(), ContractError> {
    let sha = "ab".repeat(32);
    let json = format!(
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"https://example.invalid/a\",\"sha256\":\"{sha}\"}}]}}"
    );
    let manifest = ReleaseManifest::parse_json(&json, "m.json")?;
    manifest.validate("m.json")?;
    assert!(ReleaseManifest::parse_json("not json", "m.json").is_err());
    let tampered = json.replace(&sha, &"zz".repeat(32));
    assert!(
        ReleaseManifest::parse_json(&tampered, "m.json")?
            .validate("m.json")
            .is_err()
    );
    Ok(())
}

#[test]
fn candidate_manifest_validates_all_fields() {
    let good = CandidateArtifactManifest {
        schema: 1,
        commit: "ab".repeat(20),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        toolchain: "rust@1.98.1+mr-boxington@1.19.0".to_owned(),
        sha256: "cd".repeat(32),
    };
    assert!(good.validate("c.json").is_ok());
    assert!(good.to_json_string().contains("\"toolchain\""));
    let mut bad = good.clone();
    bad.sha256 = "short".to_owned();
    assert!(bad.validate("c.json").is_err());
    bad = good.clone();
    bad.target = "wasm32-unknown-unknown".to_owned();
    assert!(bad.validate("c.json").is_err());
    bad = good;
    bad.commit = "xyz".to_owned();
    assert!(bad.validate("c.json").is_err());
}
