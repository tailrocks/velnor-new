// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const CHECK: &str = "\"$VELNOR_EXEC\" tools-verify check";
const RESET: &str = "\"$VELNOR_EXEC\" tools-verify reset";
const ANCHOR_HIT: &str = "${{ steps.toolsanchor.outputs.cache-hit }}";
const ON_DEFAULT: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";
const TRUSTED_EVENTS: &str = "(github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')";
const STORES: &[&str] = &["mbx_shared_store = true\n", "mbx_shared_store = false\n"];

fn repo_for(
    config: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    Ok(repo)
}

type Steps = Vec<(String, Vec<serde_json::Value>)>;

fn jobs_of(text: &str) -> Result<Steps, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = crate::yaml_tree::document(text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    Ok(jobs
        .iter()
        .map(|(id, job)| {
            let steps = job["steps"].as_array().cloned().unwrap_or_default();
            (id.clone(), steps)
        })
        .collect())
}

fn text(step: &serde_json::Value, field: &str) -> String {
    step[field].as_str().unwrap_or_default().to_owned()
}

fn restores_tools(step: &serde_json::Value) -> bool {
    step["name"] == "Restore the tools cache" || text(step, "run").contains("tools-local restore")
}

fn saves_tools(step: &serde_json::Value) -> bool {
    let key = step["with"]["key"].as_str().unwrap_or_default();
    (text(step, "uses").starts_with("actions/cache/save@") && key.starts_with("velnor-tools-"))
        || text(step, "run").contains("tools-local save")
}

#[test]
fn every_tools_restore_is_verified_before_anything_runs_the_tools() -> TestResult {
    for config in STORES.iter().copied() {
        let mut verified_jobs = 0;
        for (id, steps) in jobs_of(&workflow(&repo_for(config, "0.1.109")?)?)? {
            let Some(last_restore) = steps.iter().rposition(restores_tools) else {
                continue;
            };
            let verify = steps
                .iter()
                .position(|step| text(step, "run").ends_with(CHECK))
                .ok_or_else(|| format!("{config:?} {id}: no verify step"))?;
            assert!(verify > last_restore, "{config:?} {id}");
            let anchor = verify - 1;
            assert_eq!(steps[anchor]["name"], "Restore anchor", "{config:?} {id}");
            assert!(anchor > last_restore, "{config:?} {id}");
            let reset = anchor - 1;
            assert!(
                text(&steps[reset], "run").ends_with(RESET),
                "{config:?} {id}"
            );
            assert!(reset > last_restore, "{config:?} {id}");
            assert!(steps[reset].get("if").is_none(), "{config:?} {id}");
            for (between, step) in steps.iter().enumerate().take(verify).skip(last_restore + 1) {
                assert!(
                    between == reset || step.get("run").is_none(),
                    "{config:?} {id}: step {between} runs before the verify step: {step}"
                );
            }
            assert_eq!(
                steps[verify]["env"]["VELNOR_ANCHOR_HIT"], ANCHOR_HIT,
                "{config:?} {id}"
            );
            let records = steps
                .iter()
                .filter(|step| text(step, "run").ends_with("tools-verify record"));
            for record in records {
                assert_eq!(
                    record["env"]["VELNOR_ANCHOR_HIT"], ANCHOR_HIT,
                    "{config:?} {id}"
                );
                assert!(record.get("if").is_none(), "{config:?} {id}");
            }
            let install = steps
                .iter()
                .position(|step| step["name"] == "Install tools")
                .ok_or_else(|| format!("{config:?} {id}: no install"))?;
            assert!(verify < install, "{config:?} {id}");
            verified_jobs += 1;
        }
        assert_ne!(verified_jobs, 0, "{config:?}");
    }
    Ok(())
}

#[test]
fn a_pull_request_never_saves_the_tools_and_a_save_needs_the_anchor() -> TestResult {
    for config in STORES.iter().copied() {
        for helper in ["0.1.89", "0.1.109"] {
            let rendered = workflow(&repo_for(config, helper)?)?;
            let yaml: serde_json::Value = crate::yaml_tree::document(&rendered)?;
            let events: Vec<&String> = yaml["on"].as_object().ok_or("on missing")?.keys().collect();
            for event in &events {
                assert!(
                    [
                        "pull_request",
                        "push",
                        "merge_group",
                        "schedule",
                        "workflow_dispatch"
                    ]
                    .contains(&event.as_str()),
                    "{helper}: {event} could run on the default branch"
                );
            }
            let mut saves = 0;
            for (id, steps) in jobs_of(&rendered)? {
                for step in steps.iter().filter(|step| saves_tools(step)) {
                    saves += 1;
                    let condition = text(step, "if");
                    if helper == "0.1.109" {
                        let anchor = text(&step["with"], "key").starts_with("velnor-tools-anchor-");
                        let record = |output: &str| {
                            condition
                                .contains(&format!("steps.toolsrecord.outputs.{output} == 'true'"))
                        };
                        let primed = condition
                            .contains("steps.toolsverify.outputs.verified == 'true'")
                            && condition.contains(ON_DEFAULT)
                            && condition.contains(TRUSTED_EVENTS);
                        assert!(
                            if anchor {
                                record("recorded")
                            } else {
                                record("matches") || primed
                            },
                            "{helper} {id}: {condition}"
                        );
                    } else {
                        assert!(condition.contains(ON_DEFAULT), "{helper} {id}: {condition}");
                    }
                }
            }
            assert_ne!(saves, 0, "{config:?} {helper}");
        }
    }
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_unverified_restore_and_generation_says_so() -> TestResult {
    for config in STORES.iter().copied() {
        let repo = repo_for(config, "0.1.107")?;
        let rendered = workflow(&repo)?;
        assert!(!rendered.contains("tools-verify"), "{config:?}");
        let mut locked = repo.clone();
        crate::facts::lock_pinned_tools(&mut locked)?;
        let (_, notes) = expected_with_notes(&locked)?;
        assert!(
            notes
                .iter()
                .any(|note| note.contains("unverified") && note.contains("0.1.109")),
            "{config:?}: {notes:?}"
        );
        let mut current = repo_for(config, "0.1.109")?;
        crate::facts::lock_pinned_tools(&mut current)?;
        let (_, notes) = expected_with_notes(&current)?;
        assert!(
            !notes.iter().any(|note| note.contains("unverified")),
            "{config:?}"
        );
    }
    Ok(())
}

#[test]
fn every_tools_save_at_0_1_109_is_gated_to_the_default_branch() -> TestResult {
    for config in STORES.iter().copied() {
        let mut saves = 0;
        for (id, steps) in jobs_of(&workflow(&repo_for(config, "0.1.109")?)?)? {
            for step in steps.iter().filter(|step| saves_tools(step)) {
                saves += 1;
                let condition = text(step, "if");
                assert!(
                    condition.contains(ON_DEFAULT),
                    "{config:?} {id}: {condition}"
                );
            }
        }
        assert_ne!(saves, 0, "{config:?}");
    }
    Ok(())
}

#[test]
fn the_node_store_prime_copies_only_tools_the_verify_step_checked() -> TestResult {
    let config = "shape = \"groups\"\nmbx_shared_store = true\n";
    let mut primes = 0;
    for (id, steps) in jobs_of(&workflow(&repo_for(config, "0.1.109")?)?)? {
        let Some(prime) = steps
            .iter()
            .position(|step| step["name"] == "Prime the node store with the tools")
        else {
            continue;
        };
        primes += 1;
        let verify = steps
            .iter()
            .position(|step| text(step, "run").ends_with(CHECK))
            .ok_or_else(|| format!("{id}: no verify step"))?;
        assert!(verify < prime, "{id}");
        let condition = text(&steps[prime], "if");
        assert!(
            condition.contains("steps.toolsverify.outputs.verified == 'true'"),
            "{id}: {condition}"
        );
        assert!(condition.contains(ON_DEFAULT), "{id}: {condition}");
    }
    assert_ne!(primes, 0);
    Ok(())
}
