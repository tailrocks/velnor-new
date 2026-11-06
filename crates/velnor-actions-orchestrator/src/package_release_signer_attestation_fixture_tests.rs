// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const SOURCE: &str = "f1041f322c54cd7edfdef06afd307799eeaa801a";
const MANIFEST_SHA: &str = "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52";
const BINARY_SHA: &str = "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3";
const BINARY: &str = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";

#[test]
fn verifies_public_manifest_and_linux_binary() -> Result<(), Box<dyn std::error::Error>> {
    const TEST: &str = "package_release_signer::attestation::sdk::fixture_tests::verifies_public_manifest_and_linux_binary";
    if std::env::var_os("VELNOR_BINARY_FIXTURE_CHILD").is_none() {
        if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
            return super::super::image_transport_tests::dispatch(TEST);
        }
        super::super::image_transport_tests::validate(TEST)?;
        run_binary_fixture()?;
        super::super::image_transport_tests::record_success(TEST)?;
        return Ok(());
    }
    super::super::image_transport_tests::validate(TEST)?;
    let network =
        std::net::UdpSocket::bind("127.0.0.1:0").expect_err("attestation child must deny network");
    assert_eq!(network.kind(), std::io::ErrorKind::PermissionDenied);
    verify_binary_fixture()
}

fn verify_binary_fixture() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let fixture =
        std::env::var_os("VELNOR_ATTESTATION_TEST_DIR").ok_or("missing fixture directory")?;
    let root = std::path::Path::new(&fixture);
    let manifest_path = root.join("release-manifest.json");
    let binary_path = root.join(BINARY);
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(180))?;
    let manifest = verify_published_manifest_attestation(
        &manifest_path,
        &root.join("release-manifest.json.intoto.jsonl"),
        &SdkManifestAttestationExpectation {
            repository: "tailrocks/velnor-new",
            repository_id: 1_390_620_900,
            owner_id: 78_806_509,
            source_sha: SOURCE,
            manifest_sha256: MANIFEST_SHA,
        },
        deadline,
    )?;
    let bytes = std::fs::read(&manifest_path)?;
    let verified = verify_published_linux_binary_attestation(
        &binary_path,
        &root.join(format!("{BINARY}.intoto.jsonl")),
        &bytes,
        &manifest,
        deadline,
    )?;
    assert_eq!(verified.version, "0.1.5");
    assert_eq!(verified.sha256, BINARY_SHA);
    assert_eq!(verified.source_sha, SOURCE);
    assert_eq!(verified.run_id, 37_904_531_694);
    assert_eq!(verified.run_attempt, 1);
    let mut forged = bytes;
    forged.push(b' ');
    assert!(
        verify_published_linux_binary_attestation(
            &binary_path,
            &root.join(format!("{BINARY}.intoto.jsonl")),
            &forged,
            &manifest,
            deadline,
        )
        .is_err()
    );
    let temp = tempfile::tempdir()?;
    let copy = temp.path().join(BINARY);
    std::fs::copy(&binary_path, &copy)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&copy)?
        .write_all(b"x")?;
    assert!(
        verify_published_linux_binary_attestation(
            &copy,
            &root.join(format!("{BINARY}.intoto.jsonl")),
            &std::fs::read(&manifest_path)?,
            &manifest,
            deadline,
        )
        .is_err()
    );
    let bad_bundle = mutated_signature_bundle(root, temp.path())?;
    assert!(
        verify_published_linux_binary_attestation(
            &binary_path,
            &bad_bundle,
            &std::fs::read(&manifest_path)?,
            &manifest,
            deadline,
        )
        .is_err()
    );
    Ok(())
}

fn run_binary_fixture() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read as _;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let temp = tempfile::tempdir()?;
    let binary = temp.path().join(BINARY);
    let compressed = flate2::read::GzDecoder::new(std::fs::File::open(fixture.join("binary.gz"))?);
    let mut bounded = compressed.take(21_519_857);
    assert_eq!(
        std::io::copy(&mut bounded, &mut std::fs::File::create(&binary)?)?,
        21_519_856
    );
    for name in [
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        &format!("{BINARY}.intoto.jsonl"),
    ] {
        std::fs::copy(fixture.join(name), temp.path().join(name))?;
    }
    assert_eq!(
        crate::generator::sha256_hex(&std::fs::read(
            fixture.join("historical/trusted-root.jsonl")
        )?),
        "65ca537f6ed8a47fd0e560c421baa1f6c1efb8b25fc200d8c5c02c0e92eb2b9c"
    );
    let output = crate::offline_attestation_test::run_validated_worker(
        "package_release_signer::attestation::sdk::fixture_tests::verifies_public_manifest_and_linux_binary",
        "VELNOR_BINARY_FIXTURE_CHILD",
        temp.path(),
        &fixture.join("historical/trusted-root.jsonl"),
    )?;
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

fn mutated_signature_bundle(
    root: &std::path::Path,
    temp: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let bad_bundle = temp.join("invalid.intoto.jsonl");
    let mut forged_bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("{BINARY}.intoto.jsonl")))?)?;
    let signature = forged_bundle["dsseEnvelope"]["signatures"][0]["sig"]
        .as_str()
        .ok_or("signature")?;
    let mut altered = signature.as_bytes().to_vec();
    altered[0] = if altered[0] == b'A' { b'B' } else { b'A' };
    forged_bundle["dsseEnvelope"]["signatures"][0]["sig"] = String::from_utf8(altered)?.into();
    std::fs::write(&bad_bundle, serde_json::to_vec(&forged_bundle)?)?;
    Ok(bad_bundle)
}
