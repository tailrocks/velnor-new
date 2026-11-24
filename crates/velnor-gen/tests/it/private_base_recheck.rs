// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};

const BUILD_RECHECK: &str =
    "Check again that the repository is not public before the build directory is saved";
const SOURCES_RECHECK: &str =
    "Check again that the repository is not public before the sources are saved";
const FIRST: &str = "steps.private_repo.outputs.private == ''true''";
const BUILD: &str = "steps.private_recheck.outputs.private == ''true''";
const SOURCES: &str = "steps.private_recheck_sources.outputs.private == ''true''";

fn lane(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], SECTION, helper)?;
    repo.files
        .insert(path("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
    jobs_of(&workflow(&repo)?)
        .into_iter()
        .map(|(_, block)| block)
        .find(|block| block.contains("Save the sources cache"))
        .ok_or_else(|| "no lane saves the sources cache".into())
}

fn condition<'a>(lane: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    step(lane, name)
        .ok_or(name)?
        .lines()
        .find(|line| line.trim_start().starts_with("if: "))
        .ok_or_else(|| format!("{name} has no condition").into())
}

#[test]
fn the_privacy_is_looked_up_again_without_a_token_right_before_each_private_save() -> TestResult {
    let lane = lane("0.1.116")?;
    let names = names(&lane);
    let at = |name: &str| {
        names
            .iter()
            .position(|each| each == name)
            .unwrap_or(usize::MAX)
    };
    assert!(at(BUILD_RECHECK) < at("velnor-ci inputs save"), "{names:?}");
    assert!(
        at("velnor-ci inputs save") < at("Decide whether to save the build directory"),
        "{names:?}"
    );
    assert!(
        at("Fetch the sources of every root") < at(SOURCES_RECHECK)
            && at(SOURCES_RECHECK) < at("Save the sources cache"),
        "{names:?}"
    );
    for (name, id) in [
        (BUILD_RECHECK, "private_recheck"),
        (SOURCES_RECHECK, "private_recheck_sources"),
    ] {
        let block = step(&lane, name).ok_or(name)?;
        assert!(block.contains(&format!("id: {id}")), "{block}");
        assert!(block.contains("velnor-ci cache private-recheck"), "{block}");
        assert!(
            block.contains("VELNOR_API_URL: '${{ github.api_url }}'"),
            "{block}"
        );
        for token in ["GITHUB_TOKEN", "github.token", "secrets."] {
            assert!(!block.contains(token), "{token}: {block}");
        }
        assert!(block.contains(FIRST), "{block}");
    }
    Ok(())
}

#[test]
fn every_private_save_waits_for_the_fresh_verdict_and_not_for_the_first_one() -> TestResult {
    let lane = lane("0.1.116")?;
    for (name, fresh, stale) in [
        ("velnor-ci inputs save", BUILD, SOURCES),
        ("Decide whether to save the build directory", BUILD, SOURCES),
        ("Save the target cache", BUILD, SOURCES),
        ("Save the sources cache", SOURCES, BUILD),
    ] {
        let condition = condition(&lane, name)?;
        assert!(condition.contains(fresh), "{name}: {condition}");
        assert!(!condition.contains(stale), "{name}: {condition}");
        assert!(!condition.contains(FIRST), "{name}: {condition}");
    }
    let decide = step(&lane, "Decide whether to save the build directory").ok_or("no decide")?;
    assert!(
        decide
            .contains("VELNOR_REPOSITORY_PRIVATE: '${{ steps.private_recheck.outputs.private }}'"),
        "{decide}"
    );
    for name in [
        "Fetch the sources of every root",
        "Restore the target cache",
    ] {
        assert!(condition(&lane, name)?.contains(FIRST), "{name}");
    }
    Ok(())
}

#[test]
fn a_helper_without_the_recheck_keeps_the_first_verdict_on_every_save() -> TestResult {
    let lane = lane("0.1.115")?;
    assert!(!lane.contains("private_recheck"), "{lane}");
    assert!(!lane.contains("private-recheck"), "{lane}");
    for name in [
        "velnor-ci inputs save",
        "Decide whether to save the build directory",
        "Save the target cache",
        "Save the sources cache",
    ] {
        assert!(condition(&lane, name)?.contains(FIRST), "{name}");
    }
    Ok(())
}
