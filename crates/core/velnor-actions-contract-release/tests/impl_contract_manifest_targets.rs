//! Exact supported-target membership for release manifests.

use velnor_actions_contract::ContractError;
use velnor_actions_contract_release::{
    ReleaseManifest, ReleaseTarget, SUPPORTED_TARGETS, TargetRecord, asset_filename,
};

#[test]
fn release_manifest_requires_exact_supported_target_membership() -> Result<(), ContractError> {
    let good = manifest_json();
    let mut missing = ReleaseManifest::parse_json(&good, "m.json")?;
    missing.targets.pop();
    assert!(
        missing
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("missing_target"))
    );

    let mut duplicate = ReleaseManifest::parse_json(&good, "m.json")?;
    duplicate.targets.push(duplicate.targets[0].clone());
    assert!(
        duplicate
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("duplicate_target"))
    );

    let mut extra = ReleaseManifest::parse_json(&good, "m.json")?;
    extra.targets.push(TargetRecord {
        target: "aarch64-unknown-linux-gnu".to_owned(),
        artifact: "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-aarch64-unknown-linux-gnu".to_owned(),
        sha256: "ab".repeat(32),
    });
    assert!(
        extra
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("unsupported_target"))
    );

    let mut reordered = ReleaseManifest::parse_json(&good, "m.json")?;
    reordered.targets.swap(0, 1);
    reordered.validate("m.json")?;

    let mut malformed_digest = ReleaseManifest::parse_json(&good, "m.json")?;
    malformed_digest.targets[1].sha256 = "zz".repeat(32);
    assert!(
        malformed_digest
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("malformed_sha256"))
    );

    let mut malformed_url = ReleaseManifest::parse_json(&good, "m.json")?;
    malformed_url.targets[1].artifact =
        "https://evil.example/release/velnor-actions-0.1.0-aarch64-apple-darwin".to_owned();
    assert!(
        malformed_url
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("unexpected_artifact_url"))
    );
    Ok(())
}

#[test]
fn selected_target_validation_keeps_legacy_manifests_but_checks_all_records()
-> Result<(), ContractError> {
    let mut legacy = ReleaseManifest::parse_json(&manifest_json(), "m.json")?;
    legacy
        .targets
        .retain(|record| record.target != ReleaseTarget::MacosX86_64.triple());
    legacy.validate_for_targets("m.json", &[ReleaseTarget::LinuxX86_64])?;
    assert!(
        legacy
            .validate("m.json")
            .is_err_and(|error| error.to_string().contains("missing_target"))
    );
    assert!(
        legacy
            .validate_for_targets("m.json", &[ReleaseTarget::MacosX86_64])
            .is_err_and(|error| error.to_string().contains("missing_target"))
    );

    let mut duplicate = legacy.clone();
    duplicate.targets.push(duplicate.targets[0].clone());
    assert!(
        duplicate
            .validate_for_targets("m.json", &[ReleaseTarget::LinuxX86_64])
            .is_err_and(|error| error.to_string().contains("duplicate_target"))
    );

    let mut unsupported = legacy.clone();
    unsupported.targets.push(TargetRecord {
        target: "aarch64-unknown-linux-gnu".to_owned(),
        artifact: "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-aarch64-unknown-linux-gnu".to_owned(),
        sha256: "ab".repeat(32),
    });
    assert!(
        unsupported
            .validate_for_targets("m.json", &[ReleaseTarget::LinuxX86_64])
            .is_err_and(|error| error.to_string().contains("unsupported_target"))
    );

    legacy.targets[1].sha256 = "zz".repeat(32);
    assert!(
        legacy
            .validate_for_targets("m.json", &[ReleaseTarget::LinuxX86_64])
            .is_err_and(|error| error.to_string().contains("malformed_sha256"))
    );

    legacy.targets[1].sha256 = "ab".repeat(32);
    legacy.targets[1].artifact =
        "https://evil.example/release/velnor-actions-0.1.0-aarch64-apple-darwin".to_owned();
    assert!(
        legacy
            .validate_for_targets("m.json", &[ReleaseTarget::LinuxX86_64])
            .is_err_and(|error| error.to_string().contains("unexpected_artifact_url"))
    );
    Ok(())
}

fn manifest_json() -> String {
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/{}\",\"sha256\":\"{}\"}}",
                asset_filename("0.1.0", target),
                "ab".repeat(32)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "ab".repeat(20)
    )
}
