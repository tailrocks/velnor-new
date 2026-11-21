// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoView;

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const VARIABLE: &str = "VELNOR_REPOSITORY_PRIVATE";
const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const PINNED_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib#0123456789abcdef0123456789abcdef01234567\"\n";
const CRATES_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

fn repo(section: &str, helper: &str, lock: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], section, helper)?;
    repo.files
        .insert(path("Cargo.lock")?, lock.as_bytes().to_vec());
    Ok(repo)
}

fn verdicts(repo: &RepoView) -> Result<Vec<Option<String>>, Box<dyn std::error::Error>> {
    let mut found = Vec::new();
    for job in jobs(&workflow(repo)?)? {
        for step in job["steps"].as_array().into_iter().flatten() {
            if step["name"].as_str() == Some("Decide whether to save the build directory") {
                found.push(step["env"][VARIABLE].as_str().map(str::to_owned));
            }
        }
    }
    assert!(!found.is_empty(), "no cache decide step was rendered");
    Ok(found)
}

#[test]
fn a_private_lane_decide_reads_the_verdict_step_in_its_own_env() -> TestResult {
    let expected = Some("${{ steps.private_repo.outputs.private }}".to_owned());
    for found in verdicts(&repo(SECTION, "0.1.111", GIT_LOCK)?)? {
        assert_eq!(found, expected);
    }
    Ok(())
}

#[test]
fn every_other_decide_clears_the_verdict_so_github_env_cannot_supply_it() -> TestResult {
    for (section, lock) in [
        (SECTION, CRATES_LOCK),
        ("", CRATES_LOCK),
        (
            "[public_sources]\ngit = ['https://github.com/acme/shared-lib']\n",
            PINNED_LOCK,
        ),
    ] {
        for found in verdicts(&repo(section, "0.1.111", lock)?)? {
            assert_eq!(found, Some(String::new()), "{section}");
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_renders_no_verdict_variable_without_private_sources() -> TestResult {
    let repo = repo("", "0.1.110", CRATES_LOCK)?;
    assert!(!workflow(&repo)?.contains(VARIABLE));
    Ok(())
}

#[test]
fn an_older_helper_is_refused_with_private_sources() -> TestResult {
    for lock in [GIT_LOCK, CRATES_LOCK] {
        let error = workflow(&repo(SECTION, "0.1.110", lock)?)
            .err()
            .ok_or("rendered with an older helper")?;
        assert!(error.to_string().contains("0.1.111"), "{error}");
    }
    Ok(())
}
