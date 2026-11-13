// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t51_evidence_rejects_attestation_from_another_run_or_for_other_files()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let closure = closure_json();
    for (run, label) in [
        ("8/attempts/2", "another run"),
        ("7/attempts/1", "another attempt"),
    ] {
        let mut inputs = evidence_inputs();
        inputs.archive = b"not an OCI archive".to_vec();
        inputs.attestation = attestation_json(
            run,
            &inputs.archive,
            inputs.record.as_bytes(),
            closure.as_bytes(),
        );
        let (code, output, _) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
        if code == 0 {
            return Err(fail(&format!(
                "evidence program accepted an attestation from {label}"
            )));
        }
        require(
            &output,
            "Attestation certificate run identity is incorrect.",
            label,
        )?;
    }
    let mut inputs = evidence_inputs();
    inputs.attestation = attestation_json(
        "7/attempts/2",
        b"another archive",
        inputs.record.as_bytes(),
        closure.as_bytes(),
    );
    let (code, output, _) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
    if code == 0 {
        return Err(fail(
            "evidence program accepted an attestation for other files",
        ));
    }
    require(
        &output,
        "Attestation subjects are incorrect.",
        "other files",
    )?;
    Ok(())
}

fn records(parts: &[String]) -> Result<String, Box<dyn Error>> {
    let mut all = Vec::new();
    for part in parts {
        let serde_json::Value::Array(items) = serde_json::from_str(part)? else {
            return Err(fail("attestation records left their array shape"));
        };
        all.extend(items);
    }
    Ok(serde_json::Value::Array(all).to_string())
}

#[test]
fn gc1_t51_evidence_selects_the_attestation_of_this_run() -> Result<(), Box<dyn Error>> {
    if candidate_text()?
        .matches("--limit 1000 --format json")
        .count()
        != 2
        || !IDENTITY_SOURCE.contains("\nATTESTATION_LIMIT = 1000\n")
    {
        return Err(fail(
            "attestation fetch limit and truncation limit are different",
        ));
    }
    let fixture = fixture()?;
    let closure = closure_json();
    let base = evidence_inputs();
    let attest = |run: &str, archive: &[u8]| {
        attestation_json(run, archive, base.record.as_bytes(), closure.as_bytes())
    };
    let old = attest("6/attempts/1", &base.archive);
    let old_push = old.replacen("\"workflow_dispatch\"", "\"push\"", 1);
    let current = attest("7/attempts/2", &base.archive);
    let wrong_files = attest("7/attempts/2", b"another archive");
    let mut full = vec![old.clone(); 999];
    full.push(current.clone());
    for (set, label) in [
        (vec![old.clone(), old_push, current], "beside older ones"),
        (full, "in a full listing"),
    ] {
        let mut inputs = evidence_inputs();
        inputs.attestation = records(&set)?;
        let (code, output, _) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
        if code != 0 {
            return Err(fail(&format!(
                "evidence program refused this run's attestation {label}:{output}"
            )));
        }
    }
    for (set, label, message) in [
        (
            vec![old.clone()],
            "only an older run",
            "Attestation certificate run identity is incorrect.",
        ),
        (
            vec![wrong_files, old.clone()],
            "this run for other files beside an older run",
            "Attestation subjects are incorrect.",
        ),
        (
            vec![old; 1000],
            "a possibly truncated listing without this run",
            "Attestation verification output may be truncated.",
        ),
    ] {
        let mut inputs = evidence_inputs();
        inputs.attestation = records(&set)?;
        let (code, output, _) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
        if code == 0 {
            return Err(fail(&format!("evidence program accepted {label}")));
        }
        require(&output, message, label)?;
    }
    Ok(())
}
