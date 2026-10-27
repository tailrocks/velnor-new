// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Thread 4389237067: stream OCI blobs instead of retaining them all.

use super::*;
fn stream_params(root: &Path) -> Result<String, Box<dyn Error>> {
    let params = format!(
        "{{\"identity\":{{\"source\":\"{SOURCE_SHA}\",\"workflow_ref\":\"{WORKFLOW_REF}\",\"run_id\":7,\"run_attempt\":2,\"artifact_id\":9}},\"ci\":{{\"run_id\":77,\"run_attempt\":1}},\"record_source\":{{\"commit\":\"{SOURCE_SHA}\",\"ref\":\"refs/heads/main\"}}}}"
    );
    let path = root.join("stream-params.json");
    std::fs::write(&path, params)?;
    Ok(path
        .to_str()
        .ok_or_else(|| fail("params path left its string shape"))?
        .to_owned())
}

fn run_driver(
    fixture: &Fixture,
    mode: &str,
    args: &[String],
) -> Result<(i32, String), Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(&fixture.driver)
        .arg(fixture.root.path())
        .arg(mode)
        .args(args)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("streaming driver left no exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

fn stream_fixture(
    fixture: &Fixture,
    tamper_record: bool,
) -> Result<(String, String, String), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let mut record = record_json(&parts, &archive);
    if tamper_record {
        record = record.replacen("velnor.job-image-candidate.v1", "other.schema.v1", 1);
    }
    let record_path = write_bytes(fixture.root.path(), "candidate.json", record.as_bytes())?;
    let archive_path = write_bytes(fixture.root.path(), "candidate.oci.tar", &archive)?;
    let params = stream_params(fixture.root.path())?;
    Ok((record_path, archive_path, params))
}

#[test]
fn gc1_t51_streams_blobs() -> Result<(), Box<dyn Error>> {
    check_stream_contract()?;
    check_evidence_streams()?;
    let fixture = fixture()?;
    let (record_path, archive_path, params) = stream_fixture(&fixture, false)?;
    let (code, output) = run_driver(
        &fixture,
        "stream_publish",
        &[record_path, archive_path, params],
    )?;
    if code != 0 {
        return Err(fail(&format!(
            "streaming publish rejected its valid input:{output}"
        )));
    }
    require(&output, "STREAM-FLOW-OK", "streaming flow")?;
    require(&output, "PUT-STREAMED", "streaming upload")?;
    let parts = image_parts();
    require(
        &output,
        &format!(
            "digest_reference=ghcr.io/tailrocks/velnor-job@sha256:{}",
            sha256_hex(&parts.manifest)
        ),
        "streaming reference",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_stream_rejects_tampered_get() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let (record_path, archive_path, params) = stream_fixture(&fixture, false)?;
    let (code, output) = run_driver(
        &fixture,
        "stream_publish_tamper",
        &[record_path, archive_path, params],
    )?;
    if code != 0 {
        return Err(fail(&format!(
            "streaming tamper run left its exit code:{output}"
        )));
    }
    require(&output, "STREAM-FLOW-REJECTED", "tamper rejection")?;
    require(
        &output,
        "Registry retrieved bytes changed during publication.",
        "tamper message",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_stream_rejects_bytes_contract() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let (code, output) = run_driver(&fixture, "bytes_rejected", &[])?;
    if code != 0 {
        return Err(fail(&format!(
            "bytes rejection run left its exit code:{output}"
        )));
    }
    require(&output, "BYTES-REJECTED", "bytes rejection")?;
    require(
        &output,
        "temporary file path, not bytes",
        "bytes rejection message",
    )?;
    Ok(())
}
