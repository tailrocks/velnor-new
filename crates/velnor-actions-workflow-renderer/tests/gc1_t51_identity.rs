// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Residual 4383697541: wire the materialized identity validators into the
//! VERIFY and EVIDENCE programs instead of leaving them dead code.

use super::*;

const VERIFY_OPEN: &str = "const VERIFY_PROGRAM: &str = r#\"";
const VERIFY_CLOSE: &str = "\n\"#;";
const EVIDENCE_OPEN: &str = "const EVIDENCE_PROGRAM: &str = r#\"";
const EVIDENCE_CLOSE: &str = "\n\"#;";

fn extract_program(open: &str, close: &str, label: &str) -> Result<String, Box<dyn Error>> {
    let start = AUTHORITY_MODULE
        .find(open)
        .ok_or_else(|| fail(&format!("authority lost its {label}")))?
        + open.len();
    let rest = &AUTHORITY_MODULE[start..];
    let end = rest
        .find(close)
        .ok_or_else(|| fail(&format!("authority left its {label} open")))?;
    Ok(rest[..end].to_owned())
}

fn check_verify_wires_identity() -> Result<(), Box<dyn Error>> {
    let program = extract_program(VERIFY_OPEN, VERIFY_CLOSE, "verify program")?;
    for (needle, label) in [
        (
            "import job_image_authority_identity as identity_module",
            "identity import",
        ),
        (
            "identity_module.validate_same_run_candidate(",
            "same-run candidate binding",
        ),
        ("closure_path = Path(sys.argv[4])", "closure input"),
        (
            "Candidate source closure has an incorrect schema.",
            "closure schema binding",
        ),
        (
            "Candidate source closure is not the current run source.",
            "closure source binding",
        ),
    ] {
        require(&program, needle, label)?;
    }
    Ok(())
}

fn check_evidence_wires_identity() -> Result<(), Box<dyn Error>> {
    let program = extract_program(EVIDENCE_OPEN, EVIDENCE_CLOSE, "evidence program")?;
    for (needle, label) in [
        (
            "import job_image_authority_identity as identity_module",
            "identity import",
        ),
        ("identity_module.validate_identity(", "identity validation"),
        (
            "identity_module.validate_candidate_identity(",
            "candidate identity binding",
        ),
        (
            "identity_module.validate_source_inputs(",
            "source input binding",
        ),
        (
            "identity_module.validate_artifact_record(",
            "artifact record binding",
        ),
        ("identity_module.select_ci_run(", "canonical CI selection"),
        (
            "identity_module.validate_attestation_output(",
            "attestation verification",
        ),
        ("VELNOR_IDENTITY_REPOSITORY", "repository identity"),
        ("VELNOR_IDENTITY_REPOSITORY_ID", "repository id identity"),
        ("VELNOR_IDENTITY_EVENT", "event identity"),
        ("VELNOR_IDENTITY_REF", "ref identity"),
        ("ci_runs_path = Path(sys.argv[5])", "CI runs input"),
        (
            "authenticated_source_path = Path(sys.argv[6])",
            "authenticated source input",
        ),
        (
            "artifact_response_path = Path(sys.argv[7])",
            "artifact response input",
        ),
        (
            "archive_attestation_path = Path(sys.argv[8])",
            "archive attestation input",
        ),
        (
            "record_attestation_path = Path(sys.argv[9])",
            "record attestation input",
        ),
    ] {
        require(&program, needle, label)?;
    }
    Ok(())
}

fn run_verify_program(
    fixture: &Fixture,
    record_text: &str,
    archive: &[u8],
    closure_text: Option<&str>,
) -> Result<(i32, String), Box<dyn Error>> {
    let script = fixture.root.path().join("verify_program.py");
    std::fs::write(
        &script,
        extract_program(VERIFY_OPEN, VERIFY_CLOSE, "verify program")?,
    )?;
    let record_path = write_bytes(
        fixture.root.path(),
        "verify-candidate.json",
        record_text.as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "verify.oci.tar", archive)?;
    let mut command = Command::new("python3");
    command
        .arg(&script)
        .arg(fixture.root.path())
        .arg(&record_path)
        .arg(&archive_path);
    if let Some(closure) = closure_text {
        let closure_path = write_bytes(fixture.root.path(), "closure.json", closure.as_bytes())?;
        command.arg(&closure_path);
    }
    let output = command
        .env("VELNOR_EXPECTED_SOURCE", SOURCE_SHA)
        .env("VELNOR_EXPECTED_REF", "refs/heads/main")
        .env("VELNOR_EXPECTED_REPOSITORY", "tailrocks/velnor-new")
        .env("VELNOR_EXPECTED_EVENT", "workflow_dispatch")
        .env("VELNOR_EXPECTED_WORKFLOW_REF", WORKFLOW_REF)
        .env("VELNOR_EXPECTED_RUN_ID", "7")
        .env("VELNOR_EXPECTED_RUN_ATTEMPT", "2")
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("verify program left its exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

fn check_rendered_identity_steps(text: &str) -> Result<(), Box<dyn Error>> {
    for (needle, label) in [
        (
            "Fetch canonical CI runs for the candidate source",
            "CI runs step",
        ),
        ("Fetch candidate artifact record", "artifact record step"),
        ("Fetch authenticated source inputs", "source inputs step"),
        (
            "Verify candidate attestations with Sigstore",
            "attestation step",
        ),
        ("VELNOR_IDENTITY_REPOSITORY", "repository identity env"),
        ("VELNOR_IDENTITY_REPOSITORY_ID", "repository id env"),
        ("VELNOR_IDENTITY_EVENT", "event env"),
        ("VELNOR_IDENTITY_REF", "ref env"),
        ("ci.yml/runs?head_sha=", "CI runs query"),
        ("artifacts?per_page=100", "artifact query"),
        (
            "gh attestation verify \\\"candidate/job-image.oci.tar\\\"",
            "archive attestation verification",
        ),
        (
            "gh attestation verify \\\"candidate/job-image-candidate.json\\\"",
            "record attestation verification",
        ),
        ("\\\"$archive\\\" \\\"$closure\\\"", "verify closure wiring"),
    ] {
        require(text, needle, label)?;
    }
    for path in [
        ".velnor/runner.lock",
        "images/job/ubuntu-26.04/Dockerfile",
        "images/job/ubuntu-26.04/README.md",
        "images/job/ubuntu-26.04/build-test.sh",
        "images/job/ubuntu-26.04/candidate/candidate.py",
    ] {
        require(text, path, "authenticated input path")?;
    }
    require_order(
        text,
        "velnor-ci-runs.json",
        "velnor-candidate-artifact.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-candidate-artifact.json",
        "velnor-authenticated-source.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-authenticated-source.json",
        "velnor-archive-attestation.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-archive-attestation.json",
        EVIDENCE_STEP_NAME,
        "evidence input order",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_verify_binds_closure() -> Result<(), Box<dyn Error>> {
    check_verify_wires_identity()?;
    let fixture = fixture()?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record = record_json(&parts, &archive);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&closure_json()))?;
    if code != 0 {
        return Err(fail(&format!(
            "verify program rejected its bound closure:{output}"
        )));
    }
    require(
        &output,
        "fixed authority body accepted the candidate bytes",
        "closure acceptance",
    )?;
    let foreign = closure_json().replacen(SOURCE_SHA, &"b".repeat(40), 1);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&foreign))?;
    if code == 0 {
        return Err(fail("verify program accepted its foreign closure"));
    }
    require(
        &output,
        "Candidate source closure is not the current run source.",
        "closure rejection",
    )?;
    let bad_schema = closure_json().replacen(CLOSURE_SCHEMA, "other.schema.v1", 1);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&bad_schema))?;
    if code == 0 {
        return Err(fail("verify program accepted its misshapen closure"));
    }
    require(
        &output,
        "Candidate source closure has an incorrect schema.",
        "closure schema rejection",
    )?;
    let (code, _) = run_verify_program(&fixture, &record, &archive, None)?;
    if code == 0 {
        return Err(fail("verify program accepted its missing closure"));
    }
    Ok(())
}

#[test]
fn gc1_t51_evidence_wires_identity() -> Result<(), Box<dyn Error>> {
    check_evidence_wires_identity()?;
    check_rendered_identity_steps(&candidate_text()?)?;
    let fixture = fixture()?;
    let inputs = evidence_inputs();
    let (code, output, result) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
    if code != 0 {
        return Err(fail(&format!(
            "evidence program rejected its bound identity:{output}"
        )));
    }
    require(
        &output,
        "fixed authority body assembled the publication evidence",
        "evidence acceptance",
    )?;
    let checked = check_evidence_file(&fixture, &result)?;
    require(
        &checked,
        "EVIDENCE-VALID ghcr.io/tailrocks/velnor-job@sha256:",
        "evidence reference",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_evidence_rejects_unbound_identity() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let inputs = evidence_inputs();
    let foreign_record = inputs.record.replacen(SOURCE_SHA, &"b".repeat(40), 1);
    let (code, output, _) = run_evidence_program(&fixture, &inputs, &foreign_record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its foreign source"));
    }
    require(
        &output,
        "Candidate source is not the current main source.",
        "source rejection",
    )?;
    let mut bad_ci = evidence_inputs();
    bad_ci.ci_runs = bad_ci.ci_runs.replacen("\"success\"", "\"failure\"", 1);
    let (code, _, _) = run_evidence_program(&fixture, &bad_ci, &bad_ci.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its failed CI"));
    }
    let mut bad_inputs = evidence_inputs();
    bad_inputs.authenticated_source =
        bad_inputs
            .authenticated_source
            .replacen(&"0".repeat(64), &"f".repeat(64), 1);
    let (code, output, _) = run_evidence_program(&fixture, &bad_inputs, &bad_inputs.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its changed inputs"));
    }
    require(
        &output,
        "Build input bytes changed after candidate creation.",
        "inputs rejection",
    )?;
    let mut bad_attestation = evidence_inputs();
    bad_attestation.attestation =
        bad_attestation
            .attestation
            .replacen("1390620900", "1390620901", 1);
    let (code, _, _) =
        run_evidence_program(&fixture, &bad_attestation, &bad_attestation.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its foreign attestation"));
    }
    let mut bad_artifact = evidence_inputs();
    bad_artifact.artifact_response =
        bad_artifact
            .artifact_response
            .replacen("\"expired\":false", "\"expired\":true", 1);
    let (code, _, _) = run_evidence_program(&fixture, &bad_artifact, &bad_artifact.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its expired artifact"));
    }
    Ok(())
}

#[path = "gc1_t51_evidence.rs"]
mod gc1_t51_evidence;

use gc1_t51_evidence::*;
