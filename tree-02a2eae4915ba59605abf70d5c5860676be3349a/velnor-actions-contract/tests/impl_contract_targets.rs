//! Release-target and candidate-manifest cases.
use velnor_actions_contract::{
    CandidateArtifactManifest, ContractError, RELEASE_MANIFEST_FILENAME, ReleaseManifest,
    SUPPORTED_TARGETS, asset_filename, is_seed_tag_for_version, is_supported_target,
    target_for_runner_label,
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
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu\",\"sha256\":\"{sha}\"}}]}}"
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

/// One-target manifest at `version` with `repository` and `artifact`.
fn manifest_json(version: &str, repository: &str, artifact: &str) -> String {
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"{repository}\",\"targets\":[{{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"{artifact}\",\"sha256\":\"{}\"}}]}}",
        "ab".repeat(32)
    )
}

/// Bound official-asset URL for `version` and the Linux target.
fn bound_artifact(version: &str) -> String {
    format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/{}",
        asset_filename(version, "x86_64-unknown-linux-gnu")
    )
}

#[test]
fn release_manifest_binds_repository_and_artifact_urls() -> Result<(), ContractError> {
    let good = manifest_json("0.1.0", "tailrocks/velnor-new", &bound_artifact("0.1.0"));
    ReleaseManifest::parse_json(&good, "m.json")?.validate("m.json")?;
    // Wrong repository, even a lookalike, fails closed.
    for repository in [
        "",
        " tailrocks/velnor-new",
        "tailrocks/velnor-new2",
        "evil/velnor-new",
    ] {
        let json = manifest_json("0.1.0", repository, &bound_artifact("0.1.0"));
        assert!(
            ReleaseManifest::parse_json(&json, "m.json")?
                .validate("m.json")
                .is_err(),
            "repository accepted: {repository:?}"
        );
    }
    // Attacker hosts, userinfo, smuggled suffixes, and shell metacharacters.
    let evil = [
        "https://evil.example/r/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com@evil.example/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://user@github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "http://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu?x=1",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu#frag",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu ",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/$VELNOR_ASSET_URL",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/`id`",
    ];
    for artifact in evil {
        let json = manifest_json("0.1.0", "tailrocks/velnor-new", artifact);
        assert!(
            ReleaseManifest::parse_json(&json, "m.json")?
                .validate("m.json")
                .is_err(),
            "artifact accepted: {artifact:?}"
        );
    }
    // Downgrade URL shape: a 0.2.0 manifest pinning a 0.1.0 asset.
    let downgrade = manifest_json("0.2.0", "tailrocks/velnor-new", &bound_artifact("0.1.0"));
    assert!(
        ReleaseManifest::parse_json(&downgrade, "m.json")?
            .validate("m.json")
            .is_err()
    );
    // Floating `latest` tag and missing asset segment.
    for artifact in [
        "https://github.com/tailrocks/velnor-new/releases/download/latest/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0",
    ] {
        let json = manifest_json("0.1.0", "tailrocks/velnor-new", artifact);
        assert!(
            ReleaseManifest::parse_json(&json, "m.json")?
                .validate("m.json")
                .is_err(),
            "artifact accepted: {artifact:?}"
        );
    }
    Ok(())
}

/// Seed tags are namespaced (`seed/…`) yet bound to the manifest version.
///
/// Regression: the X1 validator split tag/asset at the first `/`, so it
/// rejected every real seed manifest (seed5's own manifest failed `plan`
/// with `unexpected_artifact_url`). The exact published seed5 bytes must
/// validate; version-mismatched or malformed seed tags must not.
#[test]
fn seed_tag_artifacts_bind_to_manifest_version() -> Result<(), ContractError> {
    // Exact bytes of seed/velnor-actions-0.1.0-5 seed-release-manifest.json
    // (sha256 6b3a5d71…cb05932 on the release).
    let seed5 = "{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{\"target\":\"x86_64-unknown-linux-gnu\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.1.0-5/velnor-actions-0.1.0-x86_64-unknown-linux-gnu\",\"sha256\":\"1fa12f9e5c06dcbb65b6dc9ebb4295e69606460b37c0a441a256d1b31276ff97\"}]}";
    ReleaseManifest::parse_json(seed5, "m.json")?.validate("m.json")?;
    // Uncountered seed tag of the same version also validates.
    let uncountered = manifest_json(
        "0.1.0",
        "tailrocks/velnor-new",
        "https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.1.0/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
    );
    ReleaseManifest::parse_json(&uncountered, "m.json")?.validate("m.json")?;
    // Seed grammar: exact predicate cases.
    for (tag, version, want) in [
        ("seed/velnor-actions-0.1.0", "0.1.0", true),
        ("seed/velnor-actions-0.1.0-5", "0.1.0", true),
        ("seed/velnor-actions-0.1.0-05", "0.1.0", true),
        ("seed/velnor-actions-0.2.0-5", "0.1.0", false),
        ("seed/velnor-actions-0.1.0-5", "0.2.0", false),
        ("seed/velnor-actions-0.1.0-", "0.1.0", false),
        ("seed/velnor-actions-0.1.0-x", "0.1.0", false),
        ("seed/velnor-actions-0.1.0-5x", "0.1.0", false),
        ("seed/velnor-actions-0.1.01", "0.1.0", false),
        ("seed/velnor-actions-", "0.1.0", false),
        ("seed/evil-0.1.0-5", "0.1.0", false),
        ("seed/velnor-actions-0.1.0-5/extra", "0.1.0", false),
        ("v0.1.0", "0.1.0", false),
        ("", "0.1.0", false),
    ] {
        assert_eq!(is_seed_tag_for_version(tag, version), want, "{tag:?}");
    }
    // Version-mismatched and malformed seed tags fail closed end to end.
    for artifact in [
        "https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.2.0-5/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.1.0-x/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com/tailrocks/velnor-new/releases/download/seed/evil-0.1.0-5/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
        "https://github.com/tailrocks/velnor-new/releases/download/a/b/c/velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
    ] {
        let json = manifest_json("0.1.0", "tailrocks/velnor-new", artifact);
        assert!(
            ReleaseManifest::parse_json(&json, "m.json")?
                .validate("m.json")
                .is_err(),
            "artifact accepted: {artifact:?}"
        );
    }
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
