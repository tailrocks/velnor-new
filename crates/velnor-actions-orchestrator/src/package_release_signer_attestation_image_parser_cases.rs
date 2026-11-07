// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{AttestationExpectation, validate_output_with_additional_names};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error};

pub(super) fn validate(
    bytes: &[u8],
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
) -> Result<(), Box<dyn Error>> {
    let entries: Vec<Value> = serde_json::from_slice(bytes)?;
    let [current] = entries.as_slice() else {
        return Err("expected one genuine gh report entry".into());
    };
    check_history(current, subjects, expected)?;
    check_invalid_entries(current, subjects, expected)?;
    check_uris(current, subjects, expected)?;
    check_paginated_selection(current, subjects, expected)?;
    Ok(())
}

fn check(
    entries: Vec<Value>,
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
    error: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let result = validate_output_with_additional_names(
        &serde_json::to_vec(&entries)?,
        subjects,
        &[],
        expected,
    );
    if let Some(code) = error {
        let actual = result.expect_err("parser must reject invalid report data");
        assert!(
            actual.to_string().contains(code),
            "expected {code}; got {actual}"
        );
    } else {
        assert_eq!(result?, *subjects);
    }
    Ok(())
}

fn with_uri(current: &Value, uri: &str) -> Value {
    let mut entry = current.clone();
    entry["verificationResult"]["signature"]["certificate"]["runInvocationURI"] = json!(uri);
    entry
}

fn with_invocation(
    current: &Value,
    expected: &AttestationExpectation<'_>,
    run: u64,
    attempt: u32,
) -> Value {
    with_uri(
        current,
        &format!(
            "{}/actions/runs/{run}/attempts/{attempt}",
            expected.repository_uri
        ),
    )
}

fn check_history(
    current: &Value,
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut old = with_invocation(current, expected, expected.run_id, expected.run_attempt + 1);
    old["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .ok_or("subjects")?
        .retain(|subject| subject["name"] != super::RECORD);
    let older = with_invocation(current, expected, expected.run_id - 1, 3);
    for entries in [
        vec![old.clone(), current.clone()],
        vec![current.clone(), old.clone()],
        vec![old.clone(), older.clone(), current.clone()],
        vec![current.clone(), old.clone(), older],
    ] {
        check(entries, subjects, expected, None)?;
    }
    check(
        vec![old],
        subjects,
        expected,
        Some("package_attestation_run_identity_mismatch"),
    )?;
    check(
        vec![current.clone(), current.clone()],
        subjects,
        expected,
        Some("package_attestation_ambiguous"),
    )
}

fn check_invalid_entries(
    current: &Value,
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
) -> Result<(), Box<dyn Error>> {
    for (case, code) in [
        ("predicate", "package_attestation_predicate_mismatch"),
        ("event", "package_attestation_event_mismatch"),
        ("source", "package_attestation_identity_mismatch"),
        (
            "inventory",
            "package_attestation_subject_inventory_mismatch",
        ),
        ("digest", "package_attestation_subject_inventory_invalid"),
        ("malformed", "package_attestation_output_invalid"),
    ] {
        let invalid = invalid_entry(current, case)?;
        for entries in [
            vec![invalid.clone(), current.clone()],
            vec![current.clone(), invalid],
        ] {
            check(entries, subjects, expected, Some(code))?;
        }
    }
    Ok(())
}

fn invalid_entry(current: &Value, case: &str) -> Result<Value, Box<dyn Error>> {
    let mut invalid = current.clone();
    match case {
        "predicate" => {
            invalid["verificationResult"]["statement"]["predicateType"] =
                json!("https://invalid.example/predicate")
        }
        "event" => {
            invalid["verificationResult"]["statement"]["predicate"]["buildDefinition"]["internalParameters"]
                ["github"]["event_name"] = json!("workflow_dispatch")
        }
        "source" => {
            invalid["verificationResult"]["signature"]["certificate"]["sourceRepositoryDigest"] =
                json!("c".repeat(40))
        }
        "inventory" => invalid["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("subjects")?
            .retain(|subject| subject["name"] != super::RECORD),
        "digest" => {
            let record = invalid["verificationResult"]["statement"]["subject"]
                .as_array_mut()
                .ok_or("subjects")?
                .iter_mut()
                .find(|subject| subject["name"] == super::RECORD)
                .ok_or("record subject")?;
            record["digest"]["sha256"] = json!("z".repeat(64));
        }
        "malformed" => {
            invalid["verificationResult"]["signature"]["certificate"]
                .as_object_mut()
                .ok_or("certificate")?
                .remove("runnerEnvironment");
        }
        _ => return Err("unknown parser case".into()),
    }
    Ok(invalid)
}

fn check_uris(
    current: &Value,
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
) -> Result<(), Box<dyn Error>> {
    let prefix = format!("{}/actions/runs", expected.repository_uri);
    let older_run = expected.run_id - 1;
    for uri in [
        "https://github.com/other/repo/actions/runs/1/attempts/1".to_owned(),
        format!("{prefix}/{older_run}/attempts/1?query=1"),
        format!("{prefix}/{older_run}/attempts/1#fragment"),
        format!("{prefix}/{older_run}/attempts/1/"),
        format!("{prefix}/0/attempts/1"),
        format!("{prefix}/{older_run}/attempts/0"),
        format!("{prefix}/0{older_run}/attempts/1"),
        format!("{prefix}/{older_run}/attempts/01"),
        format!("{prefix}/18446744073709551616/attempts/1"),
        format!("{prefix}/{older_run}/attempts/4294967296"),
    ] {
        let invalid = with_uri(current, &uri);
        for entries in [
            vec![invalid.clone(), current.clone()],
            vec![current.clone(), invalid],
        ] {
            check(
                entries,
                subjects,
                expected,
                Some("package_attestation_run_identity_mismatch"),
            )?;
        }
    }
    Ok(())
}

fn check_paginated_selection(
    current: &Value,
    subjects: &BTreeMap<String, String>,
    expected: &AttestationExpectation<'_>,
) -> Result<(), Box<dyn Error>> {
    let stale = (1..=100)
        .map(|offset| with_invocation(current, expected, expected.run_id - offset, 1))
        .collect::<Vec<_>>();
    let mut selected = stale.clone();
    selected[0]["verificationResult"]["statement"]["predicate"]["buildDefinition"]["internalParameters"].as_object_mut().ok_or("internal parameters")?.remove("github");
    selected.push(current.clone());
    check(selected, subjects, expected, None)?;
    let mut absent = stale;
    absent.push(with_invocation(
        current,
        expected,
        expected.run_id,
        expected.run_attempt + 1,
    ));
    check(
        absent,
        subjects,
        expected,
        Some("package_attestation_run_identity_mismatch"),
    )?;
    let mut malformed = current.clone();
    malformed["verificationResult"]["statement"]["predicate"]["buildDefinition"]["internalParameters"].as_object_mut().ok_or("internal parameters")?.remove("github");
    check(
        vec![malformed],
        subjects,
        expected,
        Some("package_attestation_output_invalid"),
    )
}
