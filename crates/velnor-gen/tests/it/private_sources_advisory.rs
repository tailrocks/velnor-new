// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};
use crate::private_sources::{SECTION, jobs, private_repo};

fn with_deny_config(
    mut repo: velnor_model::RepoView,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    for (name, text) in [("deny.toml", ""), ("mise.toml", crate::events::DENY_MISE)] {
        let name = path(name)?;
        repo.tracked_paths.insert(name.clone());
        repo.files.insert(name, text.as_bytes().to_vec());
    }
    Ok(repo)
}

#[test]
fn the_dependency_policy_job_fetches_the_advisory_db_before_the_token_and_offline_deny()
-> TestResult {
    let repo = with_deny_config(private_repo(
        &["Cargo.toml", "tools/Cargo.toml"],
        SECTION,
        "0.1.88",
    )?)?;
    let text = workflow(&repo)?;
    let mut checked = 0;
    for job in jobs(&text)? {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let Some(deny) = steps.iter().position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo-deny") && run.contains("--offline"))
        }) else {
            continue;
        };
        checked += 1;
        let fetch = steps
            .iter()
            .position(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo-deny") && run.contains(" fetch db"))
            })
            .ok_or_else(|| format!("no advisory DB fetch step: {job}"))?;
        let token = steps
            .iter()
            .position(|step| step["id"] == "private-token")
            .ok_or("private-token step missing")?;
        assert!(fetch < token && token < deny, "{job}");
        let env = steps[fetch]["env"].as_object();
        assert!(
            env.is_none_or(|env| env.keys().all(|key| {
                !key.contains("TOKEN")
                    && !key.starts_with("GIT_CONFIG")
                    && key != "CARGO_NET_OFFLINE"
            })),
            "{job}"
        );
    }
    assert_eq!(checked, 1, "{text}");
    Ok(())
}

#[test]
fn without_the_section_the_policy_job_does_not_fetch_the_advisory_db() -> TestResult {
    let repo = with_deny_config(view(&["Cargo.toml", "tools/Cargo.toml"], None)?)?;
    let text = workflow(&repo)?;
    assert!(text.contains("cargo-deny"), "{text}");
    assert!(!text.contains("fetch db"), "{text}");
    Ok(())
}

#[test]
fn the_advisory_db_is_fetched_only_when_the_checks_include_advisories() -> TestResult {
    for (checks, fetched) in [("[\"bans\"]", false), ("[\"advisories\", \"bans\"]", true)] {
        let section = format!("{SECTION}[policy]\ndeny_checks = {checks}\n");
        let repo = with_deny_config(private_repo(
            &["Cargo.toml", "tools/Cargo.toml"],
            &section,
            "0.1.109",
        )?)?;
        let text = workflow(&repo)?;
        assert!(text.contains("cargo-deny"), "{checks}: {text}");
        assert_eq!(text.contains(" fetch db"), fetched, "{checks}: {text}");
    }
    Ok(())
}
