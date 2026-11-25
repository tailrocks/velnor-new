// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::{TestResult, path};
use crate::gate_eval::{Context, runs};
use crate::pipeline::workflow;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};

struct Run {
    event: &'static str,
    actor: &'static str,
    head_repo: &'static str,
    private: &'static str,
    allowed: &'static str,
    restores: bool,
}

const REPO: &str = "acme/app";
const FORK: &str = "someone/app";

const SCENARIOS: [Run; 12] = [
    Run {
        event: "push",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "",
        restores: false,
    },
    Run {
        event: "push",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "schedule",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "",
        restores: false,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: FORK,
        private: "true",
        allowed: "",
        restores: false,
    },
    Run {
        event: "pull_request",
        actor: "dependabot[bot]",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "true",
        restores: true,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "false",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "true",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "false",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: false,
    },
];

#[test]
fn every_private_base_restore_follows_the_merge_queue_check_and_admits_a_trusted_merge_group()
-> TestResult {
    for (roots, shape) in [
        (&["Cargo.toml"][..], ""),
        (&["Cargo.toml", "tools/Cargo.toml"][..], ""),
        (
            &["Cargo.toml", "tools/Cargo.toml"][..],
            "shape = 'groups'\n",
        ),
    ] {
        let case = format!("{roots:?} {shape}");
        let mut repo = private_repo(roots, &format!("{shape}{SECTION}"), "0.1.111")?;
        for root in roots {
            repo.files.insert(
                path(root)?.sibling(&path("Cargo.lock")?),
                GIT_LOCK.as_bytes().to_vec(),
            );
        }
        let text = workflow(&repo)?;
        let mut restores = 0;
        for job in crate::private_sources::jobs(&text)? {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let position = |id: &str| steps.iter().position(|s| s["id"] == id);
            for (index, step) in steps.iter().enumerate() {
                let gated = step["name"].as_str().is_some_and(|name| {
                    ["Restore the sources cache", "Restore the target cache"].contains(&name)
                });
                if !gated {
                    continue;
                }
                restores += 1;
                let check = position("private-check").ok_or_else(|| format!("{case}:{job}"))?;
                let repository = position("private_repo").ok_or_else(|| format!("{case}:{job}"))?;
                assert!(check < index && repository < index, "{case}:{step}");
                let condition = step["if"]
                    .as_str()
                    .and_then(|text| text.strip_prefix("${{ ")?.strip_suffix(" }}"))
                    .ok_or("restore has no condition")?;
                for scenario in &SCENARIOS {
                    let mut context = Context::new(&[
                        ("github.event_name", scenario.event),
                        ("github.actor", scenario.actor),
                        ("github.repository", REPO),
                        (
                            "github.event.pull_request.head.repo.full_name",
                            scenario.head_repo,
                        ),
                        ("github.event.pull_request.user.login", scenario.actor),
                        ("steps.private_repo.outputs.private", scenario.private),
                        ("steps.private-check.outputs.allowed", scenario.allowed),
                        ("steps.plan.outcome", "success"),
                        ("steps.plan.outputs.rust_root_mode", "full"),
                    ]);
                    context.job_ok = true;
                    assert_eq!(
                        runs(condition, &context)?,
                        scenario.restores,
                        "{case}:{}:{}:{}",
                        scenario.event,
                        scenario.private,
                        scenario.allowed
                    );
                }
            }
        }
        assert!(restores > 0, "{case}:no base cache restore");
    }
    Ok(())
}

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
    let lane = lane("0.1.117")?;
    let names = names(&lane);
    let at = |name: &str| {
        names
            .iter()
            .position(|each| each == name)
            .unwrap_or(usize::MAX)
    };
    assert!(
        at("velnor-ci inputs save") < at("Decide whether to save the build directory")
            && at("Decide whether to save the build directory") < at(BUILD_RECHECK)
            && at(BUILD_RECHECK) + 1 == at("Save the target cache"),
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
fn every_private_publication_waits_for_the_fresh_verdict_and_not_for_the_first_one() -> TestResult {
    let lane = lane("0.1.117")?;
    for (name, fresh, stale) in [
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
        decide.contains("VELNOR_REPOSITORY_PRIVATE: '${{ steps.private_repo.outputs.private }}'"),
        "{decide}"
    );
    for name in [
        "Fetch the sources of every root",
        "Restore the target cache",
        "velnor-ci inputs save",
        "Decide whether to save the build directory",
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
