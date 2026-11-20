// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoView;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::lint_gate::{needs_of, spec, verdict};
use crate::lint_lane::{LINT, repo, runs};
use crate::lock::{COMMIT_FILE, SUMS};
use crate::lock_archive::archive_lock_at;
use crate::pipeline::workflow;

pub(crate) const CHECK: &str = "[[lint]]\nkind = 'generated-check'\n";
/// The first helper that reads `kind = "generated-check"`; every check fixture pins it.
pub(crate) const CHECK_HELPER: &str = "0.1.107";
const ID: &str = "lint-generated-check";
const X64: &str = "2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9";
const ARM64: &str = "300f6b8a0d89cfe0643433710d7df3453c3524e7ff84c7dfc84854a78c7e120c";

fn locked(config: &str, lock: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = repo(config)?;
    view.tracked_paths.insert(LOCK_PATH);
    view.files.insert(LOCK_PATH, lock.as_bytes().to_vec());
    Ok(view)
}

fn rendered(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&locked(config, &archive_lock_at(CHECK_HELPER)?)?)
}

fn job(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"][ID].clone())
}

#[test]
fn the_check_job_runs_on_every_event_with_nothing_but_read_access_to_the_pull_request() -> TestResult
{
    let text = rendered(&format!("{LINT}\n{CHECK}"))?;
    let job = job(&text)?;
    assert!(job.is_object(), "{text}");
    assert_eq!(job["permissions"], serde_json::json!({"contents": "read"}));
    assert!(job["needs"].is_null() && job["environment"].is_null());
    assert!(!job.to_string().contains("secrets"), "{job}");
    let condition = job["if"].as_str().ok_or("no job condition")?;
    for selector in [
        "draft",
        "labels",
        "label.name",
        "merge_group",
        "skip",
        "outputs",
    ] {
        assert!(!condition.contains(selector), "{selector}: {condition}");
    }
    assert!(
        job["steps"]
            .as_array()
            .ok_or("no steps")?
            .iter()
            .all(|step| step["if"].is_null()),
        "{job}"
    );
    let steps = job["steps"].as_array().ok_or("no steps")?;
    assert_eq!(steps[0]["with"]["persist-credentials"], false);
    assert!(
        steps[0]["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with("actions/checkout@"))
    );
    assert_eq!(
        runs(&job).last().map(String::as_str),
        Some("velnor-gen check")
    );
    Ok(())
}

#[test]
fn the_check_runs_the_pinned_release_after_its_digest_is_verified_and_never_a_build_of_the_pull_request()
-> TestResult {
    let text = rendered(CHECK)?;
    let job = job(&text)?;
    let install = job["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .find(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("Install velnor-gen"))
        })
        .ok_or("no install step")?;
    let env = &install["env"];
    assert_eq!(
        env["VELNOR_GEN_RELEASE"],
        "https://github.com/tailrocks/velnor-new/releases/download/tools-v0.1.107"
    );
    let archive = env["VELNOR_GEN_ARCHIVE"].as_str().ok_or("no archive")?;
    let digest = env["VELNOR_GEN_SHA256"].as_str().ok_or("no digest")?;
    for (arch, triple, sum) in [("X64", "x86_64", X64), ("ARM64", "aarch64", ARM64)] {
        assert!(
            archive.contains(&format!(
                "runner.arch == '{arch}' && 'velnor-gen-{triple}-unknown-linux-musl.tar.gz'"
            )),
            "{archive}"
        );
        assert!(
            digest.contains(&format!("runner.arch == '{arch}' && '{sum}'")),
            "{digest}"
        );
    }
    let script = env["VELNOR_GEN_INSTALL"].as_str().ok_or("no script")?;
    assert!(
        script.contains(r#"[ "$actual" = "$VELNOR_GEN_SHA256" ] || fail"#),
        "{script}"
    );
    assert!(
        script
            .find("--output")
            .zip(script.find("tar --extract"))
            .is_some_and(|(get, extract)| get < extract)
    );
    assert!(
        script
            .find("SHA256")
            .zip(script.find("tar --extract"))
            .is_some_and(|(check, extract)| check < extract)
    );
    let names = job["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !names
            .iter()
            .any(|name| name.contains("cargo") || name.contains("Build")),
        "{names:?}"
    );
    assert!(!text.contains("cargo build") && !text.contains("cargo install"));
    Ok(())
}

#[test]
fn required_fails_when_the_check_fails_is_cancelled_is_skipped_or_is_missing() -> TestResult {
    let text = rendered(&format!("{LINT}\n{CHECK}"))?;
    assert!(needs_of(&text)?.contains(&ID.to_owned()));
    let gate = spec(&text)?;
    let listed = gate
        .jobs
        .iter()
        .find(|job| job.id.to_string() == ID)
        .ok_or("not in the gate spec")?;
    assert!(listed.when.is_empty(), "the check is expected on every run");
    let (green, table) = verdict(&text, |_| Some("success"))?;
    assert!(green, "{table}");
    for result in ["failure", "cancelled", "skipped"] {
        let (green, table) = verdict(&text, |id| Some(if id == ID { result } else { "success" }))?;
        assert!(!green && table.contains(ID), "{result}: {table}");
    }
    let (green, table) = verdict(&text, |id| (id != ID).then_some("success"))?;
    assert!(!green, "{table}");
    Ok(())
}

#[test]
fn the_check_is_off_unless_a_lint_entry_asks_for_it() -> TestResult {
    assert!(job(&rendered(LINT)?)?.is_null());
    Ok(())
}

#[test]
fn a_lock_without_archive_digests_cannot_install_velnor_gen_so_generation_stops() -> TestResult {
    let raw = velnor_rust::pin_lock(CHECK_HELPER, COMMIT_FILE, SUMS, None)?;
    let result = expected_files(&locked(CHECK, &raw)?);
    assert!(
        matches!(result, Err(GenerateError::GeneratorNotLocked(_))),
        "{result:?}"
    );
    Ok(())
}
