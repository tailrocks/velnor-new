// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use velnor_model::RepoPath;

#[test]
fn refused_base_caches_say_why_when_the_helper_reads_the_reason() -> TestResult {
    let lock = "version = 4\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#abc\"\n";
    for (helper, reason) in [
        ("0.1.110", None),
        (
            "0.1.111",
            Some("private dependencies; declare [private_sources]"),
        ),
    ] {
        let mut repo = crate::base_save::pinned(helper, "")?;
        repo.files
            .insert(RepoPath::new("Cargo.lock")?, lock.as_bytes().to_vec());
        let parsed: serde_json::Value =
            serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(&repo)?))?;
        let mut checked = 0;
        for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
            for report in job["steps"]
                .as_array()
                .ok_or("steps")?
                .iter()
                .filter(|step| step["run"] == "velnor-ci report")
            {
                let caches: serde_json::Value = serde_json::from_str(
                    report["env"]["VELNOR_REPORT_CACHES"]
                        .as_str()
                        .ok_or("cache descriptors")?,
                )?;
                for cache in caches.as_array().ok_or("caches")? {
                    if cache["family"] == "tools" {
                        assert!(cache.get("not_generated").is_none(), "{cache}");
                        continue;
                    }
                    checked += 1;
                    assert_eq!(
                        cache.get("not_generated").and_then(|value| value.as_str()),
                        reason,
                        "{helper}: {cache}"
                    );
                }
            }
        }
        assert!(checked > 0, "{helper}");
    }
    Ok(())
}

#[test]
fn every_save_step_the_report_names_is_rendered_in_its_job() -> TestResult {
    let lock = "version = 4\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#abc\"\n";
    let mut repo = crate::base_save::pinned("0.1.111", "")?;
    repo.files
        .insert(RepoPath::new("Cargo.lock")?, lock.as_bytes().to_vec());
    let parsed: serde_json::Value =
        serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(&repo)?))?;
    let mut named = 0;
    for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
        let steps = job["steps"].as_array().ok_or("steps")?;
        let ids: Vec<&str> = steps
            .iter()
            .filter_map(|step| step["id"].as_str())
            .collect();
        for report in steps
            .iter()
            .filter(|step| step["run"] == "velnor-ci report")
        {
            let caches: serde_json::Value = serde_json::from_str(
                report["env"]["VELNOR_REPORT_CACHES"]
                    .as_str()
                    .ok_or("cache descriptors")?,
            )?;
            for cache in caches.as_array().ok_or("caches")? {
                if let Some(save) = cache["save_step"].as_str() {
                    named += 1;
                    assert!(
                        ids.contains(&save),
                        "{save} is named but not rendered: {cache}"
                    );
                }
            }
        }
    }
    assert!(named > 0);
    Ok(())
}
