// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
const SOURCE: &str = "f1041f322c54cd7edfdef06afd307799eeaa801a";
const MANIFEST_SHA: &str = "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52";
const BINARY_SHA: &str = "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3";
const BINARY: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
const MANIFEST: &[u8] = include_bytes!("../tests/attestation_fixtures/manifest-verified.json");
const EXECUTABLE: &[u8] = include_bytes!("../tests/attestation_fixtures/binary-verified.json");
fn parse(
    raw: &[u8],
    name: &str,
    sha: &str,
    binary: bool,
) -> Result<VerifiedManifestAttestation, OrchestratorError> {
    parse_verified_release_attestation(
        raw,
        "tailrocks/velnor-new",
        1_390_620_900,
        78_806_509,
        SOURCE,
        &SubjectExpectation {
            name,
            sha256: sha,
            binary,
        },
    )
}
#[test]
fn verified_subjects_are_bound_to_exact_release_and_run() -> Result<(), Box<dyn std::error::Error>>
{
    let manifest = parse_verified_manifest_attestation(
        MANIFEST,
        "tailrocks/velnor-new",
        1_390_620_900,
        78_806_509,
        SOURCE,
        MANIFEST_SHA,
    )?;
    let executable = parse(EXECUTABLE, BINARY, BINARY_SHA, true)?;
    assert!(same_release_run(&manifest, &executable));
    let bytes = include_bytes!("../tests/attestation_fixtures/release-manifest.json");
    bound_release_manifest(bytes, &manifest)?;
    let mut altered = bytes.to_vec();
    altered.push(b' ');
    assert!(bound_release_manifest(&altered, &manifest).is_err());
    let mut changed_run = executable.clone();
    changed_run.run_id += 1;
    assert!(!same_release_run(&manifest, &changed_run));
    let other_run = std::str::from_utf8(EXECUTABLE)?.replace("37904531694", "37904531695");
    let other = parse(other_run.as_bytes(), BINARY, BINARY_SHA, true)?;
    assert!(!same_release_run(&manifest, &other));
    assert!(parse(EXECUTABLE, BINARY, MANIFEST_SHA, true).is_err());
    assert!(
        parse(
            EXECUTABLE,
            "velnor-actions-0.1.5-aarch64-apple-darwin",
            BINARY_SHA,
            true
        )
        .is_err()
    );
    assert!(parse(EXECUTABLE, BINARY, BINARY_SHA, false).is_err());
    assert!(parse(MANIFEST, BINARY, BINARY_SHA, true).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
    let subjects = value[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .ok_or("subjects")?;
    subjects.push(subjects[0].clone());
    assert!(parse(&serde_json::to_vec(&value)?, BINARY, BINARY_SHA, true).is_err());
    let mut missing: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
    missing[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .ok_or("subjects")?
        .pop();
    assert!(parse(&serde_json::to_vec(&missing)?, BINARY, BINARY_SHA, true).is_err());
    let mut invalid: serde_json::Value = serde_json::from_slice(EXECUTABLE)?;
    invalid[0]["verificationResult"]["statement"]["subject"][1]["digest"]["sha256"] =
        serde_json::json!("not-a-sha");
    assert!(parse(&serde_json::to_vec(&invalid)?, BINARY, BINARY_SHA, true).is_err());
    Ok(())
}

#[test]
fn rejects_wrong_attestation_identity() {
    assert!(
        parse_verified_release_attestation(
            EXECUTABLE,
            "tailrocks/other",
            1_390_620_900,
            78_806_509,
            SOURCE,
            &SubjectExpectation {
                name: BINARY,
                sha256: BINARY_SHA,
                binary: true
            }
        )
        .is_err()
    );
    assert!(
        parse_verified_release_attestation(
            EXECUTABLE,
            "tailrocks/velnor-new",
            1_390_620_901,
            78_806_509,
            SOURCE,
            &SubjectExpectation {
                name: BINARY,
                sha256: BINARY_SHA,
                binary: true
            }
        )
        .is_err()
    );
    assert!(
        parse_verified_release_attestation(
            EXECUTABLE,
            "tailrocks/velnor-new",
            1_390_620_900,
            78_806_509,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &SubjectExpectation {
                name: BINARY,
                sha256: BINARY_SHA,
                binary: true
            }
        )
        .is_err()
    );
}

fn check_optional_public_tool_measurements(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    generator: &AttestedLinuxGenerator,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(root) = std::env::var_os("VELNOR_PUBLIC_TOOL_FIXTURE") {
        check_public_tool_measurements(std::path::Path::new(&root), intent, receipt, generator)?;
    }
    Ok(())
}

fn check_public_tool_measurements(
    source: &std::path::Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &crate::crate_release_journal::JournalReceipt,
    generator: &AttestedLinuxGenerator,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let paths = [
        "velnor/bootstrap/velnor-actions-0.1.5-x86_64-unknown-linux-gnu",
        "velnor-release-plz/release-plz",
        "velnor/rust-manifest/channel.toml",
    ];
    for path in paths {
        let target = root.path().join(path);
        std::fs::create_dir_all(target.parent().ok_or("parent")?)?;
        std::fs::copy(source.join(path), target)?;
    }
    assert_eq!(
        measure_release_tools(root.path(), intent, receipt, generator)?.into_pins(),
        receipt.expected_tools
    );
    for path in paths {
        let target = root.path().join(path);
        std::fs::write(&target, b"tampered")?;
        assert!(measure_release_tools(root.path(), intent, receipt, generator).is_err());
        std::fs::copy(source.join(path), target)?;
    }
    Ok(())
}

#[test]
fn expected_tool_pins_match_reviewed_intent_and_attested_generator()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalAuthentication, JournalIdentity, JournalReceipt, JournalRegistry, JournalToolPins,
    };
    let intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":null}),
    )?;
    let mut generator = AttestedLinuxGenerator {
        version: "0.1.5".into(),
        sha256: BINARY_SHA.into(),
        source_sha: SOURCE.into(),
        run_id: 37_904_531_694,
        run_attempt: 1,
    };
    let release_plz = velnor_actions_mise::catalog::qualified_release_plz("0.4.3", "ubuntu-24.04")
        .ok_or("catalog")?;
    let rust =
        velnor_actions_mise::catalog::rust_channel_manifest_sha256("1.97.1").ok_or("rust")?;
    let mut receipt = JournalReceipt {
        semver_evidence: std::collections::BTreeMap::new(),
        predecessor: None,
        identity: JournalIdentity {
            repository: "example/demo".into(),
            workflow_sha: "a".repeat(40),
            source_sha: "b".repeat(40),
            run_id: 12,
            run_attempt: 1,
        },
        intent_sha256: "c".repeat(64),
        base_intent_sha256: "c".repeat(64),
        registry: JournalRegistry::CratesIo,
        generator_version: "0.1.5".into(),
        release_plz_version: "0.4.3".into(),
        rust_version: "1.97.1".into(),
        expected_tools: JournalToolPins {
            generator_sha256: BINARY_SHA.into(),
            release_plz_sha256: release_plz.executable_sha256.into(),
            rust_manifest_sha256: rust.into(),
        },
        authentication: JournalAuthentication::TrustedPublishing,
        required_check_id: 1,
        trusted_reviewer_id: 139_017,
        workflow_run_url: "https://github.com/example/demo/actions/runs/12".into(),
        source_files: vec![],
        packages: vec![],
    };
    let derived = derive_attested_tool_expectations(&intent, &generator)?;
    assert_eq!(derived, receipt.expected_tools);
    match_attested_tool_expectations(&intent, &receipt, &generator)?;
    check_optional_public_tool_measurements(&intent, &receipt, &generator)?;
    generator.source_sha = "d".repeat(40);
    assert!(derive_attested_tool_expectations(&intent, &generator).is_err());
    generator.source_sha = SOURCE.into();
    receipt.expected_tools.generator_sha256 = "0".repeat(64);
    assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
    receipt.expected_tools.generator_sha256 = BINARY_SHA.into();
    receipt.expected_tools.release_plz_sha256 = "0".repeat(64);
    assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
    receipt.expected_tools.release_plz_sha256 = release_plz.executable_sha256.into();
    receipt.expected_tools.rust_manifest_sha256 = "0".repeat(64);
    assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
    receipt.expected_tools.rust_manifest_sha256 = rust.into();
    let mut invalid = intent.clone();
    invalid.runner = "ubuntu-22.04".into();
    assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
    invalid = intent.clone();
    invalid.generator_version = "0.1.6".into();
    assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
    invalid = intent.clone();
    invalid.release_plz_version = "0.4.4".into();
    assert!(match_attested_tool_expectations(&invalid, &receipt, &generator).is_err());
    receipt.generator_version = "0.1.6".into();
    assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
    receipt.generator_version = "0.1.5".into();
    receipt.release_plz_version = "0.4.4".into();
    assert!(match_attested_tool_expectations(&intent, &receipt, &generator).is_err());
    check_wrong_rust_version(&intent, &mut receipt, &generator);
    Ok(())
}

fn check_wrong_rust_version(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    receipt: &mut crate::crate_release_journal::JournalReceipt,
    generator: &AttestedLinuxGenerator,
) {
    receipt.release_plz_version = "0.4.3".into();
    receipt.rust_version = "1.97.2".into();
    assert!(match_attested_tool_expectations(intent, receipt, generator).is_err());
}
