//! Release-manifest target coverage and published asset URL cases.
use velnor_actions_contract::{
    RELEASE_MANIFEST_FILENAME, ReleaseManifest, SUPPORTED_TARGETS, TargetRecord, asset_filename,
    check_release_artifact,
};

fn valid_manifest() -> ReleaseManifest {
    ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: "0.1.0".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        commit: "ab".repeat(20),
        targets: SUPPORTED_TARGETS
            .iter()
            .map(|target| TargetRecord {
                target: (*target).to_owned(),
                artifact: format!(
                    "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/{}",
                    asset_filename("0.1.0", target)
                ),
                sha256: "ab".repeat(32),
            })
            .collect(),
    }
}

#[test]
fn release_manifest_requires_every_supported_target_once() {
    let mut missing = valid_manifest();
    missing.targets.pop();
    let error = missing
        .validate("m.json")
        .expect_err("missing target rejected");
    assert!(
        error
            .to_string()
            .contains(&format!("missing_target:{}", SUPPORTED_TARGETS[2])),
        "{error}"
    );

    let mut duplicate = valid_manifest();
    duplicate.targets[2].target = duplicate.targets[0].target.clone();
    let error = duplicate
        .validate("m.json")
        .expect_err("duplicate target rejected");
    assert!(
        error
            .to_string()
            .contains(&format!("duplicate_target:{}", SUPPORTED_TARGETS[0])),
        "{error}"
    );

    let mut unsupported = valid_manifest();
    unsupported.targets[2].target = "wasm32-unknown-unknown".to_owned();
    let error = unsupported
        .validate("m.json")
        .expect_err("unsupported target rejected");
    assert!(error.to_string().contains("unsupported_target"), "{error}");
}

#[test]
fn release_manifest_asset_url_uses_canonical_name() {
    let manifest = valid_manifest();
    let canonical = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/{RELEASE_MANIFEST_FILENAME}"
    );
    let check = |url: &str| manifest.validate_published_asset_url(url, "m.json");
    assert!(check(&canonical).is_ok());

    let seed = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.1.0-5/{RELEASE_MANIFEST_FILENAME}"
    );
    assert!(check(&seed).is_ok());

    for invalid in [
        canonical.replace(RELEASE_MANIFEST_FILENAME, "other-manifest.json"),
        canonical.replace("github.com", "evil.example"),
        canonical.replace("v0.1.0", "latest"),
        format!("{canonical}?download=1"),
        format!(
            "https://github.com/tailrocks/velnor-new/releases/download/seed/velnor-actions-0.2.0-5/{RELEASE_MANIFEST_FILENAME}"
        ),
    ] {
        assert!(
            check(&invalid).is_err(),
            "manifest asset URL accepted: {invalid}"
        );
    }
}

#[test]
fn release_asset_tags_are_explicit_and_normalization_safe() {
    let manifest = valid_manifest();
    let check_binary = |url: &str| {
        check_release_artifact(
            url,
            &manifest.version,
            &manifest.commit,
            SUPPORTED_TARGETS[0],
            "m.json",
            "targets.artifact",
        )
    };
    let generator_tag = format!("generator-{}", manifest.commit);
    let accepted_tags = [
        "v0.1.0".to_owned(),
        generator_tag,
        "seed/velnor-actions-0.1.0".to_owned(),
        "seed/velnor-actions-0.1.0-5".to_owned(),
    ];
    for tag in accepted_tags {
        let binary = tagged_asset_url(&tag, &asset_filename("0.1.0", SUPPORTED_TARGETS[0]));
        let published_manifest = tagged_asset_url(&tag, RELEASE_MANIFEST_FILENAME);
        assert!(check_binary(&binary).is_ok(), "valid tag rejected: {tag}");
        assert!(
            manifest
                .validate_published_asset_url(&published_manifest, "m.json")
                .is_ok(),
            "valid manifest URL tag rejected: {tag}"
        );
    }

    let mut rejected_tags = vec![
        ".",
        "..",
        "%2e",
        "%2E%2e",
        "%2f",
        "%5c",
        "%252e%252e",
        "v0.1.0/..",
        "../v0.1.0",
        "v0.1.0%2f..%2f",
        r"..\v0.1.0",
        "seed//velnor-actions-0.1.0",
        "stable",
        "latest",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    rejected_tags.push(format!("generator-{}", "ab".repeat(19)));
    rejected_tags.push(format!("generator-{}", "AB".repeat(20)));
    rejected_tags.push(format!("generator-{}", "cd".repeat(20)));
    for tag in rejected_tags {
        let binary = tagged_asset_url(&tag, &asset_filename("0.1.0", SUPPORTED_TARGETS[0]));
        let published_manifest = tagged_asset_url(&tag, RELEASE_MANIFEST_FILENAME);
        assert!(check_binary(&binary).is_err(), "unsafe tag accepted: {tag}");
        assert!(
            manifest
                .validate_published_asset_url(&published_manifest, "m.json")
                .is_err(),
            "unsafe manifest URL tag accepted: {tag}"
        );
    }
}

fn tagged_asset_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/tailrocks/velnor-new/releases/download/{tag}/{asset}")
}
