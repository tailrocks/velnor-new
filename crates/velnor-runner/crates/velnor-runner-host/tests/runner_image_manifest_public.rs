// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{Value, json};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_SOURCE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const LOCK: &str = "runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n";

type TestResult<T> = Result<T, Box<dyn Error>>;

fn image(repository: &str, digest: char) -> Value {
    let digest = format!("sha256:{}", digest.to_string().repeat(64));
    json!({
        "repository": repository,
        "tag": format!("sha-{SOURCE}"),
        "registry_digest": digest.clone(),
        "platform_manifest_digest": digest,
        "config_digest": format!("sha256:{}", "e".repeat(64))
    })
}

fn descriptors() -> Value {
    json!({
        "schema_version": 1,
        "source_commit": SOURCE,
        "platform": "linux/amd64",
        "images": {
            "runner": image("ghcr.io/tailrocks/velnor-runner", '1'),
            "dind": image("ghcr.io/tailrocks/velnor-dind", '2'),
            "resource_probe": image("ghcr.io/tailrocks/velnor-resource-probe", '3')
        }
    })
}

fn invoke(
    temp: &tempfile::TempDir,
    descriptor_bytes: &[u8],
    lock: &str,
    source: &str,
    authority: &str,
    existing_output: Option<&[u8]>,
) -> TestResult<(Output, PathBuf)> {
    let descriptor_path = temp.path().join("descriptors.json");
    let lock_path = temp.path().join("runner.lock");
    let output_path = temp.path().join("manifest.json");
    fs::write(&descriptor_path, descriptor_bytes)?;
    fs::write(&lock_path, lock)?;
    if let Some(bytes) = existing_output {
        fs::write(&output_path, bytes)?;
    }
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .env_clear()
        .env("VELNOR_SOURCE_SHA", source)
        .env("VELNOR_WORKFLOW_AUTHORITY_SHA", authority)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_SHA", source)
        .arg("write-runner-image-manifest")
        .arg(descriptor_path)
        .arg(lock_path)
        .arg(&output_path)
        .output()?;
    Ok((output, output_path))
}

struct InvalidCase {
    name: &'static str,
    descriptors: Vec<u8>,
    lock: String,
    source: &'static str,
    authority: &'static str,
}

fn input_case(
    name: &'static str,
    descriptors: &Value,
    lock: String,
    source: &'static str,
    authority: &'static str,
) -> TestResult<InvalidCase> {
    Ok(InvalidCase {
        name,
        descriptors: serde_json::to_vec(descriptors)?,
        lock,
        source,
        authority,
    })
}

fn default_failure_case(name: &'static str, descriptors: &Value) -> TestResult<InvalidCase> {
    input_case(name, descriptors, LOCK.to_owned(), SOURCE, SOURCE)
}

fn source_and_reference_failure_cases() -> TestResult<Vec<InvalidCase>> {
    let mut cases = Vec::new();
    let mut value = descriptors();
    value["source_commit"] = json!(OTHER_SOURCE);
    cases.push(default_failure_case("descriptor source mismatch", &value)?);

    let mut value = descriptors();
    value["images"]["runner"]["repository"] = json!("ghcr.io/tailrocks/wrong");
    cases.push(default_failure_case("wrong repository", &value)?);

    let mut value = descriptors();
    value["images"]["dind"]["tag"] = json!("latest");
    cases.push(default_failure_case("wrong tag", &value)?);

    let mut value = descriptors();
    value["platform"] = json!("linux/arm64");
    cases.push(default_failure_case("wrong platform", &value)?);
    Ok(cases)
}

fn inventory_and_digest_failure_cases() -> TestResult<Vec<InvalidCase>> {
    let mut cases = Vec::new();
    let mut value = descriptors();
    value["images"]
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("images object"))?
        .remove("runner");
    cases.push(default_failure_case("missing component", &value)?);

    let mut value = descriptors();
    value["images"]["archive"] = image("ghcr.io/tailrocks/archive", '4');
    cases.push(default_failure_case("extra component", &value)?);

    let mut value = descriptors();
    value["unknown"] = json!(true);
    cases.push(default_failure_case("unknown field", &value)?);

    let mut value = descriptors();
    value["images"]["runner"]["config_digest"] = json!(format!("sha256:{}", "z".repeat(64)));
    cases.push(default_failure_case("invalid digest", &value)?);

    let mut value = descriptors();
    value["images"]["runner"]["registry_digest"] = json!(format!("sha256:{}", "9".repeat(64)));
    cases.push(default_failure_case(
        "registry and platform digest differ",
        &value,
    )?);
    Ok(cases)
}

fn descriptor_failure_cases() -> TestResult<Vec<InvalidCase>> {
    let mut cases = source_and_reference_failure_cases()?;
    cases.extend(inventory_and_digest_failure_cases()?);
    Ok(cases)
}

fn source_and_lock_failure_cases() -> TestResult<Vec<InvalidCase>> {
    let mut cases = vec![input_case(
        "workflow authority mismatch",
        &descriptors(),
        LOCK.to_owned(),
        SOURCE,
        OTHER_SOURCE,
    )?];
    cases.push(input_case(
        "missing lock version",
        &descriptors(),
        LOCK.replace("node_version = \"26.12.0\"\n", ""),
        SOURCE,
        SOURCE,
    )?);
    let bytes = serde_json::to_vec(&descriptors())?;
    let duplicate = String::from_utf8(bytes)?.replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    cases.push(InvalidCase {
        name: "duplicate field",
        descriptors: duplicate.into_bytes(),
        lock: LOCK.to_owned(),
        source: SOURCE,
        authority: SOURCE,
    });
    Ok(cases)
}

#[test]
fn public_write_runner_manifest_formats_exact_candidate_schema() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let bytes = serde_json::to_vec(&descriptors())?;
    let (output, manifest_path) = invoke(&temp, &bytes, LOCK, SOURCE, SOURCE, None)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    assert_eq!(manifest["schema_version"], 2);
    assert_eq!(manifest["repository"], "tailrocks/velnor-new");
    assert_eq!(manifest["source_ref"], "refs/heads/main");
    assert_eq!(manifest["source_commit"], SOURCE);
    assert_eq!(
        manifest["signer_workflow"],
        ".github/workflows/product-release-images.yml"
    );
    assert_eq!(manifest["signer_ref"], "refs/heads/main");
    assert_eq!(manifest["workflow_authority_sha"], SOURCE);
    assert_eq!(
        manifest["trusted_signing_identity"]["oidc_issuer"],
        "https://token.actions.githubusercontent.com"
    );
    assert_eq!(
        manifest["trusted_signing_identity"]["certificate_identity"],
        concat!(
            "https://github.com/tailrocks/velnor-new/",
            ".github/workflows/product-release-images.yml@refs/heads/main"
        )
    );
    assert_eq!(manifest["platform"], "linux/amd64");
    assert_eq!(manifest["versions"]["runner"], "2.338.0");
    assert_eq!(manifest["versions"]["docker"], "29.9.1");
    assert_eq!(manifest["versions"]["buildx"], "0.38.0");
    assert_eq!(manifest["versions"]["compose"], "5.6.0");
    assert_eq!(manifest["versions"]["node"], "26.12.0");
    assert_eq!(manifest["images"], descriptors()["images"]);
    assert_eq!(manifest["resource_probe"]["protocol_version"], 1);
    assert_eq!(manifest["resource_probe"]["image_user"], "65532:65532");
    assert_eq!(
        manifest["resource_probe"]["entrypoint"],
        "/velnor/resource-probe"
    );
    assert_eq!(manifest["resource_probe"]["working_directory"], "/");
    Ok(())
}

#[test]
fn public_write_runner_manifest_rejects_invalid_inputs_without_output() -> TestResult<()> {
    let cases = descriptor_failure_cases()?
        .into_iter()
        .chain(source_and_lock_failure_cases()?);
    for case in cases {
        let temp = tempfile::tempdir()?;
        let (output, output_path) = invoke(
            &temp,
            &case.descriptors,
            &case.lock,
            case.source,
            case.authority,
            None,
        )?;
        assert!(!output.status.success(), "accepted {}", case.name);
        assert!(!output_path.exists(), "created output for {}", case.name);
    }
    Ok(())
}

#[test]
fn public_write_runner_manifest_preserves_existing_output() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let bytes = serde_json::to_vec(&descriptors())?;
    let (output, manifest_path) = invoke(
        &temp,
        &bytes,
        LOCK,
        SOURCE,
        SOURCE,
        Some(b"preserve this file"),
    )?;
    assert!(!output.status.success());
    assert_eq!(fs::read(manifest_path)?, b"preserve this file");
    Ok(())
}
