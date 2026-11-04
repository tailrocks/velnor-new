// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{WitnessRead, WitnessRequest, verify_witness};
#[path = "crate_release_qualification_witness/fixtures.rs"]
mod fixtures;
use fixtures::Fixture;

#[test]
fn witness_requires_unique_marker_and_exact_run_job_artifact_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut f = Fixture::new()?;
    assert_eq!(verify_witness(&f.request, &f)?.artifact_id, 77);
    let original = f.log.clone();
    f.log.pop();
    assert!(verify_witness(&f.request, &f).is_err());
    f.log = String::from_utf8(original.clone())?
        .replace("2026-11-05T00:00:00.000Z", "bad-time")
        .into_bytes();
    assert!(verify_witness(&f.request, &f).is_err());
    f.log = original;
    f.log.extend(f.log.clone());
    assert!(verify_witness(&f.request, &f).is_err());
    f.log = String::from_utf8(f.log)?
        .lines()
        .next()
        .ok_or("missing log")?
        .replace("artifact=77", "artifact=077")
        .into_bytes();
    f.log.push(b'\n');
    assert!(verify_witness(&f.request, &f).is_err());
    Ok(())
}

#[test]
fn witness_rejects_wrong_approved_source_or_artifact_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut f = Fixture::new()?;
    f.request.candidate_sha = "d".repeat(40);
    assert!(verify_witness(&f.request, &f).is_err());
    f.request.candidate_sha = "c".repeat(40);
    f.bytes[0] ^= 1;
    assert!(verify_witness(&f.request, &f).is_err());
    Ok(())
}

#[test]
fn witness_rejects_unsuccessful_or_unrelated_jobs_and_runs()
-> Result<(), Box<dyn std::error::Error>> {
    let f = Fixture::new()?;
    f.job.borrow_mut()["conclusion"] = "failure".into();
    assert!(verify_witness(&f.request, &f).is_err());
    f.job.borrow_mut()["conclusion"] = "success".into();
    f.run.borrow_mut()["event"] = "pull_request".into();
    assert!(verify_witness(&f.request, &f).is_err());
    Ok(())
}

#[test]
fn witness_rejects_artifact_that_changes_during_acquisition()
-> Result<(), Box<dyn std::error::Error>> {
    let f = Fixture::new()?;
    f.change_artifact_after_download.set(true);
    assert!(verify_witness(&f.request, &f).is_err());
    Ok(())
}
