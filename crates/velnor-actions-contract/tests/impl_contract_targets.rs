//! Release-target and candidate-manifest cases.
use velnor_actions_contract::config::{
    LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG, is_valid_rust_target,
};
use velnor_actions_contract::{
    CandidateArtifactManifest, ContractError, RELEASE_MANIFEST_FILENAME, ReleaseManifest,
    ReleaseTarget, SUPPORTED_TARGETS, asset_filename, is_seed_tag_for_version, is_supported_target,
};

#[test]
fn supported_targets_and_naming() {
    assert_eq!(
        SUPPORTED_TARGETS,
        [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
        ]
    );
    assert!(is_supported_target("x86_64-unknown-linux-gnu"));
    assert!(!is_supported_target("wasm32-unknown-unknown"));
    assert_eq!(RELEASE_MANIFEST_FILENAME, "release-manifest.json");
    assert_eq!(
        asset_filename("0.1.0", "x86_64-unknown-linux-gnu"),
        "velnor-actions-0.1.0-x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        ReleaseTarget::for_runner_label("ubuntu-26.04"),
        Some(ReleaseTarget::LinuxX86_64)
    );
    assert!(ReleaseTarget::for_runner_label("ubuntu-26.04-arm").is_none());
}

#[test]
fn target_ids_round_trip_without_ordinal_mapping() {
    for target in ReleaseTarget::ALL {
        assert_eq!(ReleaseTarget::parse_triple(target.triple()), Some(target));
    }
    assert_eq!(ReleaseTarget::ALL.len(), SUPPORTED_TARGETS.len());
    assert!(ReleaseTarget::parse_triple("aarch64-unknown-linux-gnu").is_none());
}

#[test]
fn versioned_macos_labels_bind_explicit_architecture() {
    for label in ["macos-14", "macos-15", "macos-15-arm64", "macos-26"] {
        assert_eq!(
            ReleaseTarget::for_runner_label(label),
            Some(ReleaseTarget::MacosArm64)
        );
    }
    for label in ["macos-15-intel", "macos-26-intel"] {
        assert_eq!(
            ReleaseTarget::for_runner_label(label),
            Some(ReleaseTarget::MacosX86_64)
        );
    }
    for label in ["macos-latest", "macos-13", "custom-macos", "macos-15-arm"] {
        assert!(ReleaseTarget::for_runner_label(label).is_none(), "{label}");
    }
}

#[test]
fn runner_label_catalog_maps_or_fails_closed() {
    // Every config-accepted label either maps to a supported release
    // target or maps to nothing; `None` labels hard-fail generation
    // (`bad_label`, `unsupported_target_for_runner`) instead of
    // silently taking the build host's arch/OS.
    assert_eq!(RUNNER_LABEL_CATALOG.len(), 6);
    assert!(RUNNER_LABEL_CATALOG.contains(&LATEST_RUNNER_LABEL));
    for label in RUNNER_LABEL_CATALOG {
        match ReleaseTarget::for_runner_label(label) {
            Some(target) => {
                assert!(
                    is_supported_target(target.triple()),
                    "{label} maps to supported {}",
                    target.triple()
                );
                assert!(!label.ends_with("-arm"), "{label} is an x64 label");
            }
            None => assert!(
                label.ends_with("-arm"),
                "only -arm catalog labels fail mapping: {label}"
            ),
        }
    }
    assert!(is_supported_target(
        ReleaseTarget::for_runner_label(LATEST_RUNNER_LABEL)
            .expect("latest maps")
            .triple()
    ));
    // Rust execution targets are charset-validated, not allowlisted:
    // supported configs CAN carry non-release triples, so generation
    // must hard-fail on them (platform identity, cache keys) rather
    // than mis-executing. See `sources_cache_key` and
    // `platform_inputs_for` rejection tests.
    for other in [
        "host",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(is_valid_rust_target(other), "{other} is config-accepted");
    }
    assert!(!is_supported_target("aarch64-unknown-linux-gnu"));
    assert!(!is_valid_rust_target("x86_64 unknown"));
    assert!(!is_valid_rust_target("aarch64-${{ x }}"));
}

#[test]
fn release_manifest_json_round_trip_and_tamper() -> Result<(), ContractError> {
    let sha = "ab".repeat(32);
    let commit = "ab".repeat(20);
    let json = manifest_json(
        "0.1.0",
        "tailrocks/velnor-new",
        &bound_artifact("0.1.0", ReleaseTarget::LinuxX86_64.triple()),
    );
    assert!(json.contains(&commit));
    assert!(json.contains(&sha));
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

/// Complete manifest at `version` with a configurable Linux artifact URL.
fn manifest_json(version: &str, repository: &str, artifact: &str) -> String {
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            let target_artifact = if *target == ReleaseTarget::LinuxX86_64.triple() {
                artifact.to_owned()
            } else {
                bound_artifact(version, target)
            };
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"{target_artifact}\",\"sha256\":\"{}\"}}",
                "ab".repeat(32)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"{repository}\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "ab".repeat(20)
    )
}

/// Bound official-asset URL for `version` and `target`.
fn bound_artifact(version: &str, target: &str) -> String {
    format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/{}",
        asset_filename(version, target)
    )
}

/// Complete test manifest at one release tag, with a shared digest.
fn manifest_json_with_tag(version: &str, tag: &str, sha256: &str) -> String {
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/{tag}/{}\",\"sha256\":\"{sha256}\"}}",
                asset_filename(version, target)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "ab".repeat(20)
    )
}

#[test]
fn release_manifest_binds_repository_and_artifact_urls() -> Result<(), ContractError> {
    let good = manifest_json(
        "0.1.0",
        "tailrocks/velnor-new",
        &bound_artifact("0.1.0", ReleaseTarget::LinuxX86_64.triple()),
    );
    ReleaseManifest::parse_json(&good, "m.json")?.validate("m.json")?;
    // Wrong repository, even a lookalike, fails closed.
    for repository in [
        "",
        " tailrocks/velnor-new",
        "tailrocks/velnor-new2",
        "evil/velnor-new",
    ] {
        let json = manifest_json(
            "0.1.0",
            repository,
            &bound_artifact("0.1.0", ReleaseTarget::LinuxX86_64.triple()),
        );
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
    let downgrade = manifest_json(
        "0.2.0",
        "tailrocks/velnor-new",
        &bound_artifact("0.1.0", ReleaseTarget::LinuxX86_64.triple()),
    );
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

#[test]
fn release_manifest_requires_commit() -> Result<(), ContractError> {
    let good = manifest_json(
        "0.1.0",
        "tailrocks/velnor-new",
        &bound_artifact("0.1.0", ReleaseTarget::LinuxX86_64.triple()),
    );
    let manifest = ReleaseManifest::parse_json(&good, "m.json")?;
    manifest.validate("m.json")?;
    assert_eq!(manifest.commit, "ab".repeat(20));
    // Missing commit rejected at parse.
    let segment = format!("\"commit\":\"{}\",", "ab".repeat(20));
    let missing = good.replace(&segment, "");
    assert!(
        !missing.contains("\"commit\""),
        "fixture must drop the field"
    );
    let err = ReleaseManifest::parse_json(&missing, "m.json").expect_err("commit required");
    assert!(err.to_string().contains("commit"), "{err}");
    // Malformed commit rejected at validation.
    for bad in [
        String::new(),
        "xyz".to_owned(),
        "A".repeat(40),
        "a".repeat(39),
    ] {
        let json = good.replace(&segment, &format!("\"commit\":\"{bad}\","));
        assert!(
            ReleaseManifest::parse_json(&json, "m.json")?
                .validate("m.json")
                .is_err_and(|err| err.to_string().contains("malformed_commit")),
            "commit accepted: {bad:?}"
        );
    }
    Ok(())
}

/// Seed tags are namespaced (`seed/…`) yet bound to the manifest version.
///
/// Regression: the X1 validator split tag/asset at the first `/`, so it
/// rejected every real seed manifest (seed5's own manifest failed `plan`
/// with `unexpected_artifact_url`). Seed grammar binding must validate;
/// version-mismatched or malformed seed tags must not.
#[test]
fn seed_tag_artifacts_bind_to_manifest_version() -> Result<(), ContractError> {
    // Historical one-target seed manifests are incomplete; seed tags still
    // bind correctly when a manifest carries every supported target.
    let seed5 = manifest_json_with_tag(
        "0.1.0",
        "seed/velnor-actions-0.1.0-5",
        "1fa12f9e5c06dcbb65b6dc9ebb4295e69606460b37c0a441a256d1b31276ff97",
    );
    ReleaseManifest::parse_json(&seed5, "m.json")?.validate("m.json")?;
    // Uncountered seed tag of the same version also validates.
    let uncountered =
        manifest_json_with_tag("0.1.0", "seed/velnor-actions-0.1.0", &"ab".repeat(32));
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
