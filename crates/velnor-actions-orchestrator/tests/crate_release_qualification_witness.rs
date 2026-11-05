// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{WitnessRead, WitnessRequest, verify_witness};
use crate::crate_release_qualification_bundle::acquire_metadata;
use crate::crate_release_qualification_bundle::acquire_output_bundle;
#[path = "crate_release_qualification_witness/fixtures.rs"]
mod fixtures;
use fixtures::Fixture;

#[test]
fn metadata_requires_exact_witness_source_and_artifact_custody()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Cursor, Write};
    let mut f = Fixture::new()?;
    f.request.metadata = Some(
        velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
            manifest_path: "Cargo.toml".into(),
            intent_config_path: ".velnor/config.toml".into(),
            qualification: velnor_actions_contract::config::ReleaseQualificationPolicy {
                candidate_repository: "candidate/repo".into(),
                candidate_repository_id: 1,
                candidate_default_branch: "main".into(),
                candidate_required_workflow_path: ".github/workflows/ci.yml".into(),
                review_proof:
                    velnor_actions_contract::config::ReleaseReviewProof::CompletionCheck {
                        completion_app_id: 1,
                        completion_check_name: "complete".into(),
                        reviewer_app_ids: [1].into_iter().collect(),
                    },
                historical_manifest:
                    velnor_actions_contract::config::ReleaseHistoricalManifestPolicy {
                        version: "0.1.5".into(),
                        source_sha: "b".repeat(40),
                        sha256: "c".repeat(64),
                    },
            },
            rust_version: "1.97.1".into(),
        },
    );
    f.request.source_sha = Some("d".repeat(40));
    let mut job = f.job.borrow_mut();
    let steps = job["steps"].as_array_mut().ok_or("steps")?;
    for name in ["Bind metadata artifact", "Bind approved metadata source"] {
        steps.push(serde_json::json!({"name": name, "status":"completed", "conclusion":"success"}));
    }
    drop(job);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.start_file("metadata.json", zip::write::SimpleFileOptions::default())?;
    archive.write_all(b"{}")?;
    f.bytes = archive.finish()?.into_inner();
    let digest = crate::generator::sha256_hex(&f.bytes);
    f.artifact.borrow_mut()["name"] = "approved-metadata-observation".into();
    f.artifact.borrow_mut()["size_in_bytes"] = f.bytes.len().into();
    f.artifact.borrow_mut()["digest"] = format!("sha256:{digest}").into();
    f.log = format!("2026-11-05T00:00:00Z VELNOR_METADATA_SOURCE run=33 attempt=1 source={} config=.velnor/config.toml tree={} sha256={}\n2026-11-05T00:00:00Z VELNOR_REHEARSAL_METADATA run=33 attempt=1 artifact=77 sha256={digest}\n", "d".repeat(40), "e".repeat(40), "f".repeat(64)).into_bytes();
    let evidence = acquire_metadata(&f.request, &f)?;
    assert_eq!(evidence.bytes(), b"{}");
    assert_eq!(evidence.source_sha(), "d".repeat(40));
    assert_eq!(evidence.source_tree_sha(), "e".repeat(40));
    let original = f.log.clone();
    f.log.extend(original.clone());
    assert!(acquire_metadata(&f.request, &f).is_err());
    f.log = String::from_utf8(original.clone())?
        .replace("source=dddd", "source=aaaa")
        .into_bytes();
    assert!(acquire_metadata(&f.request, &f).is_err());
    f.log = original;
    f.request
        .metadata
        .as_mut()
        .ok_or("metadata")?
        .intent_config_path = "other/config.toml".into();
    assert!(acquire_metadata(&f.request, &f).is_err());
    f.request
        .metadata
        .as_mut()
        .ok_or("metadata")?
        .intent_config_path = ".velnor/config.toml".into();
    f.job.borrow_mut()["steps"][2]["conclusion"] = "skipped".into();
    assert!(acquire_metadata(&f.request, &f).is_err());
    f.job.borrow_mut()["steps"][2]["conclusion"] = "success".into();
    f.change_artifact_after_download.set(true);
    assert!(acquire_metadata(&f.request, &f).is_err());
    Ok(())
}

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

#[test]
fn output_bundle_requires_witness_custody_and_exact_bytes() -> Result<(), Box<dyn std::error::Error>>
{
    use std::io::{Cursor, Write};
    let mut f = Fixture::new()?;
    f.request.output_bundle = true;
    f.job.borrow_mut()["steps"].as_array_mut().ok_or("steps")?.push(serde_json::json!({"name":"Bind rehearsal output artifact","status":"completed","conclusion":"success"}));
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.start_file(
        "rehearsal-output.tar",
        zip::write::SimpleFileOptions::default(),
    )?;
    archive.write_all(b"untrusted tar")?;
    f.bytes = archive.finish()?.into_inner();
    let digest = crate::generator::sha256_hex(&f.bytes);
    f.artifact.borrow_mut()["name"] = "rehearsal-output-observation".into();
    f.artifact.borrow_mut()["size_in_bytes"] = f.bytes.len().into();
    f.artifact.borrow_mut()["digest"] = format!("sha256:{digest}").into();
    f.log = format!("2026-11-05T00:00:00Z VELNOR_REHEARSAL_OUTPUT run=33 attempt=1 artifact=77 sha256={digest}\n").into_bytes();
    assert_eq!(acquire_output_bundle(&f.request, &f)?, b"untrusted tar");
    f.log.extend(f.log.clone());
    assert!(acquire_output_bundle(&f.request, &f).is_err());
    Ok(())
}
