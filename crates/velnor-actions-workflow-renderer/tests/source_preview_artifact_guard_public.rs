// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use serde_json::Value;
use std::error::Error;
use std::process::Command;

use super::rendered;

fn workflow_texts() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut texts: Vec<(String, String)> = rendered()?
        .into_iter()
        .filter(|file| file.path.starts_with(".github/workflows/"))
        .map(|file| (file.path, file.bytes))
        .collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows");
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "yml") {
            texts.push((
                format!("disk:{}", path.display()),
                std::fs::read_to_string(&path)?,
            ));
        }
    }
    Ok(texts)
}

type Guard = (String, String, String);

fn guarded_downloads() -> Result<Vec<Guard>, Box<dyn Error>> {
    let mut found = Vec::new();
    for (path, text) in workflow_texts()? {
        let workflow: Value = serde_saphyr::from_str_with_options(
            &text,
            serde_saphyr::options! { budget: serde_saphyr::budget! { enforce_alias_anchor_ratio: false } },
        )?;
        for (job, body) in workflow["jobs"].as_object().into_iter().flatten() {
            let steps = body["steps"].as_array().map_or(&[][..], Vec::as_slice);
            for (index, step) in steps.iter().enumerate() {
                let Some(id) = step["with"]["artifact-ids"].as_str() else {
                    continue;
                };
                let guard = index
                    .checked_sub(1)
                    .and_then(|previous| steps.get(previous))
                    .ok_or_else(|| format!("{path} {job}: download {id} has no guard step"))?;
                let env = guard["env"].as_object().ok_or_else(|| {
                    format!("{path} {job}: download {id} is not preceded by a guard")
                })?;
                let by_env = id
                    .strip_prefix("${{ env.")
                    .and_then(|name| name.strip_suffix(" }}"))
                    .is_some_and(|name| env.contains_key(name));
                assert!(
                    by_env || env.values().any(|value| value == id),
                    "{path} {job}: step before download {id} does not check that id"
                );
                assert_eq!(guard.get("if"), step.get("if"));
                if env.contains_key("VELNOR_EVIDENCE_ARTIFACT_ID") {
                    let run = guard["run"].as_str().ok_or("guard run")?;
                    found.push((path.clone(), job.clone(), run.to_owned()));
                }
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
    assert!(found.len() >= 40, "found {} guarded downloads", found.len());
    Ok(())
}

#[test]
fn guard_step_fails_for_empty_and_non_numeric_ids_and_passes_for_positive_integers()
-> Result<(), Box<dyn Error>> {
    for (path, job, run) in guarded_downloads()? {
        for bad in [
            "",
            "abc",
            "0",
            "-1",
            "1 2",
            "01",
            "1a",
            "1\n2",
            "9007199254740992",
            "12345678901234567",
        ] {
            assert!(
                !run_guard(&run, bad)?,
                "{path} {job}: guard accepted {bad:?}"
            );
        }
        for good in ["1", "42", "9876543210", "9007199254740991"] {
            assert!(
                run_guard(&run, good)?,
                "{path} {job}: guard rejected {good:?}"
            );
        }
    }
    Ok(())
}
