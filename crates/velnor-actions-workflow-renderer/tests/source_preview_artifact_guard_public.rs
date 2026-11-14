// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use serde_json::Value;
use std::error::Error;
use std::process::Command;

use super::{find_file, rendered};

const WORKFLOWS: [&str; 2] = [
    ".github/workflows/product-release-generator.yml",
    ".github/workflows/source-preview-publication.yml",
];

fn guarded_downloads() -> Result<Vec<(String, String, String)>, Box<dyn Error>> {
    let files = rendered()?;
    let mut found = Vec::new();
    for path in WORKFLOWS {
        let workflow: Value = serde_saphyr::from_str(find_file(&files, path)?)?;
        for (job, body) in workflow["jobs"].as_object().ok_or("jobs")? {
            if !job.starts_with("source-preview-") {
                continue;
            }
            let steps = body["steps"].as_array().map_or(&[][..], Vec::as_slice);
            for (index, step) in steps.iter().enumerate() {
                let Some(id) = step["with"]["artifact-ids"].as_str() else {
                    continue;
                };
                let guard = index
                    .checked_sub(1)
                    .and_then(|previous| steps.get(previous))
                    .ok_or_else(|| format!("{path} {job}: download {id} has no guard step"))?;
                assert_eq!(
                    guard["name"], "Validate exact artifact ID",
                    "{path} {job}: download {id} is not preceded by the guard"
                );
                assert_eq!(guard["env"]["VELNOR_EVIDENCE_ARTIFACT_ID"], id);
                assert_eq!(guard.get("if"), step.get("if"));
                let run = guard["run"].as_str().ok_or("guard run")?;
                found.push((path.to_owned(), job.clone(), run.to_owned()));
            }
        }
    }
    Ok(found)
}

fn run_guard(run: &str, id: &str) -> Result<bool, Box<dyn Error>> {
    let status = Command::new("/bin/bash")
        .args(["-c", run])
        .env_clear()
        .env("VELNOR_EVIDENCE_ARTIFACT_ID", id)
        .status()?;
    Ok(status.success())
}

#[test]
fn every_download_by_id_is_preceded_by_a_guard_in_the_same_job() -> Result<(), Box<dyn Error>> {
    let found = guarded_downloads()?;
    assert!(found.len() >= 10, "found {} guarded downloads", found.len());
    Ok(())
}

#[test]
fn guard_step_fails_for_empty_and_non_numeric_ids_and_passes_for_positive_integers()
-> Result<(), Box<dyn Error>> {
    for (path, job, run) in guarded_downloads()? {
        for bad in ["", "abc", "0", "-1", "1 2", "01", "1a", "1\n2"] {
            assert!(
                !run_guard(&run, bad)?,
                "{path} {job}: guard accepted {bad:?}"
            );
        }
        for good in ["1", "42", "9876543210"] {
            assert!(
                run_guard(&run, good)?,
                "{path} {job}: guard rejected {good:?}"
            );
        }
    }
    Ok(())
}
