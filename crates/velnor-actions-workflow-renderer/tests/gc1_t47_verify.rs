// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const VERIFY_OPEN: &str = "const VERIFY_PROGRAM: &str = r#\"";
const VERIFY_CLOSE: &str = "\n\"#;";

pub(crate) fn verify_program() -> Result<String, Box<dyn Error>> {
    let start = AUTHORITY_MODULE
        .find(VERIFY_OPEN)
        .ok_or_else(|| fail("authority lost its verify program"))?
        + VERIFY_OPEN.len();
    let rest = &AUTHORITY_MODULE[start..];
    let end = rest
        .find(VERIFY_CLOSE)
        .ok_or_else(|| fail("authority left its verify program open"))?;
    Ok(rest[..end].to_owned())
}

pub(crate) fn verify_record_json(parts: &ImageParts, archive: &[u8]) -> String {
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    format!(
        "{{\"schema\":\"velnor.job-image-candidate.v1\",\"source\":{{\"commit\":\"{commit}\",\"ref\":\"refs/heads/main\",\"inputs\":{{\".velnor/runner.lock\":\"{lock}\",\"images/job/ubuntu-26.04/Dockerfile\":\"{lock}\",\"images/job/ubuntu-26.04/README.md\":\"{lock}\",\"images/job/ubuntu-26.04/build-test.sh\":\"{lock}\",\"images/job/ubuntu-26.04/candidate/candidate.py\":\"{lock}\"}}}}",
        commit = SOURCE_SHA,
        lock = "0".repeat(64),
    ) + &format!(
        ",\"workflow\":{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1,\"event\":\"push\",\"workflow_ref\":\"{workflow_ref}\",\"authority_commit\":\"{commit}\",\"run_id\":1,\"run_attempt\":1}},\"platform\":\"linux/amd64\",\"build\":{{\"builder\":\"github.com/docker/buildx v0.25.0\",\"platform\":\"linux/amd64\",\"arguments\":[],\"exporter\":\"oci\",\"provenance\":\"disabled\",\"compression\":\"gzip\",\"force_compression\":true}},\"archive\":{{\"sha256\":\"{archive_sha}\",\"size_bytes\":{archive_len}}},\"oci\":{{\"archive_index_digest\":\"{index}\",\"archive_index_size_bytes\":{index_len},\"platform_manifest_digest\":\"sha256:{manifest_hex}\",\"platform_manifest_size_bytes\":{manifest_len},\"image_configuration_digest\":\"sha256:{config_hex}\",\"image_configuration_size_bytes\":{config_len},\"layer_digests\":[\"sha256:{layer_hex}\"]}}}}",
        workflow_ref = WORKFLOW_REF,
        commit = SOURCE_SHA,
        archive_sha = sha256_hex(archive),
        archive_len = archive.len(),
        index = sha256_hex(&parts.index),
        index_len = parts.index.len(),
        manifest_hex = manifest_hex,
        manifest_len = parts.manifest.len(),
        config_hex = config_hex,
        config_len = parts.config.len(),
        layer_hex = layer_hex,
    )
}

pub(crate) fn run_verify_program(
    fixture: &Fixture,
    record_text: &str,
    archive: &[u8],
) -> Result<(i32, String), Box<dyn Error>> {
    let script = fixture.root.path().join("verify_program.py");
    std::fs::write(&script, verify_program()?)?;
    let record_path = write_bytes(
        fixture.root.path(),
        "verify-candidate.json",
        record_text.as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "verify.oci.tar", archive)?;
    let output = Command::new("python3")
        .arg(&script)
        .arg(fixture.root.path())
        .arg(&record_path)
        .arg(&archive_path)
        .env("VELNOR_EXPECTED_SOURCE", SOURCE_SHA)
        .env("VELNOR_EXPECTED_REF", "refs/heads/main")
        .env("VELNOR_EXPECTED_REPOSITORY", "tailrocks/velnor-new")
        .env("VELNOR_EXPECTED_EVENT", "push")
        .env("VELNOR_EXPECTED_WORKFLOW_REF", WORKFLOW_REF)
        .env("VELNOR_EXPECTED_RUN_ID", "1")
        .env("VELNOR_EXPECTED_RUN_ATTEMPT", "1")
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("verify program left its exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

#[test]
fn gc1_t47_verify_program_binds_source() -> Result<(), Box<dyn Error>> {
    forbid(
        AUTHORITY_MODULE,
        "archive_path.read_bytes()",
        "archive streaming",
    )?;
    require(
        AUTHORITY_MODULE,
        "while chunk := archive_stream.read(1048576):",
        "archive streaming",
    )?;
    require(AUTHORITY_MODULE, "VELNOR_EXPECTED_SOURCE", "source binding")?;
    require(
        AUTHORITY_MODULE,
        "Candidate source is not the current run source.",
        "source binding",
    )?;
    require(
        AUTHORITY_MODULE,
        "Candidate workflow is not the current run workflow.",
        "source binding",
    )?;
    let rendered = candidate_text()?;
    require(&rendered, "VELNOR_EXPECTED_SOURCE", "verify wiring")?;
    require(&rendered, "VELNOR_EXPECTED_WORKFLOW_REF", "verify wiring")?;
    require(
        &rendered,
        "Verify candidate bytes with the fixed authority body",
        "verify wiring",
    )?;
    let fixture = fixture()?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record = verify_record_json(&parts, &archive);
    let (code, output) = run_verify_program(&fixture, &record, &archive)?;
    if code != 0 {
        return Err(fail(&format!(
            "verify program rejected its bound source:{output}"
        )));
    }
    require(
        &output,
        "fixed authority body accepted the candidate bytes",
        "source acceptance",
    )?;
    let foreign_commit = record.replacen(SOURCE_SHA, &"b".repeat(40), 1);
    let (code, output) = run_verify_program(&fixture, &foreign_commit, &archive)?;
    if code == 0 {
        return Err(fail("verify program accepted its foreign source"));
    }
    require(
        &output,
        "Candidate source is not the current run source.",
        "source rejection",
    )?;
    let foreign_workflow = record.replacen(
        WORKFLOW_REF,
        "tailrocks/velnor-new/.github/workflows/other.yml@refs/heads/main",
        1,
    );
    let (code, output) = run_verify_program(&fixture, &foreign_workflow, &archive)?;
    if code == 0 {
        return Err(fail("verify program accepted its foreign workflow"));
    }
    require(
        &output,
        "Candidate workflow is not the current run workflow.",
        "workflow rejection",
    )?;
    let tampered_archive = tamper_layer_data(&archive, &parts);
    let (code, _) = run_verify_program(&fixture, &record, &tampered_archive)?;
    if code == 0 {
        return Err(fail("verify program accepted its tampered archive"));
    }
    Ok(())
}
