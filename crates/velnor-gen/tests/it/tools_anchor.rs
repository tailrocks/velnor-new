// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const HELPER: &str = "0.1.110";
const PLAN: &str = "rust-plan";
const PLAN_ANCHOR: &str = "${{ needs.rust-plan.outputs.tools_anchor }}";
const READ_ANCHOR: &str = "${{ steps.toolsread.outputs.anchor }}";
const RECORDED: &str = "${{ steps.toolsrecord.outputs.digest }}";
const STORES: &[&str] = &["mbx_shared_store = true\n", "mbx_shared_store = false\n"];
const PREFIX: &str = "velnor-tools-anchor-${{ runner.os }}-${{ runner.arch }}-mise2026.11.10-${{ hashFiles('mise.lock') }}-";
const TOOLS_KEY: &str = "velnor-tools-${{ runner.os }}-${{ runner.arch }}-mise2026.11.10-${{ hashFiles('mise.lock') }}-";

fn render(
    manifests: &[&str],
    config: &str,
    helper: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut repo = view(manifests, Some(config.as_bytes()))?;
    pin_helper(&mut repo, helper)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?)
}

fn steps(job: &Value) -> Vec<Value> {
    job["steps"].as_array().cloned().unwrap_or_default()
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

fn jobs(yaml: &Value) -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>> {
    Ok(yaml["jobs"]
        .as_object()
        .ok_or("jobs missing")?
        .iter()
        .map(|(id, job)| (id.clone(), job.clone()))
        .collect())
}

fn named<'a>(steps: &'a [Value], name: &str) -> Option<&'a Value> {
    steps.iter().find(|step| step["name"] == name)
}

fn position(steps: &[Value], pick: impl Fn(&Value) -> bool) -> Option<usize> {
    steps.iter().position(pick)
}

fn restores_tools(step: &Value) -> bool {
    step["name"] == "Restore the tools cache" || text(step, "run").contains("tools-local restore")
}

fn verifies(step: &Value) -> bool {
    text(step, "run").ends_with("tools-verify check")
}

#[test]
fn the_plan_job_reads_the_anchor_once_and_every_job_takes_its_digest_from_it() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, HELPER)?;
        let mut verified = 0;
        for (id, job) in jobs(&yaml)? {
            let steps = steps(&job);
            let finds = steps
                .iter()
                .filter(|step| step["name"] == "Find the anchor")
                .count();
            let reads = steps
                .iter()
                .filter(|step| step["name"] == "Read the anchor")
                .count();
            if id == PLAN {
                assert_eq!((finds, reads), (1, 1), "{config:?}");
                assert_eq!(job["outputs"]["tools_anchor"], READ_ANCHOR, "{config:?}");
                continue;
            }
            assert_eq!((finds, reads), (0, 0), "{config:?} {id}");
            assert!(job["outputs"]["tools_anchor"].is_null(), "{config:?} {id}");
            for step in &steps {
                let run = text(step, "run");
                assert!(!run.contains("tools-verify reset"), "{config:?} {id}");
                assert!(step["name"] != "Restore anchor", "{config:?} {id}");
            }
            let Some(verify) = position(&steps, verifies) else {
                continue;
            };
            verified += 1;
            assert!(
                text(&job, "needs").contains(PLAN) || job["needs"].to_string().contains(PLAN),
                "{config:?} {id}: {}",
                job["needs"]
            );
            assert_eq!(
                steps[verify]["env"]["VELNOR_TOOLS_ANCHOR"], PLAN_ANCHOR,
                "{config:?} {id}"
            );
            let restore = position(&steps, restores_tools).ok_or("no restore")?;
            assert!(restore < verify, "{config:?} {id}");
            let install =
                position(&steps, |step| step["name"] == "Install tools").ok_or("no install")?;
            assert!(verify < install, "{config:?} {id}");
        }
        assert_ne!(verified, 0, "{config:?}");
    }
    Ok(())
}

#[test]
fn the_plan_job_finds_the_newest_anchor_by_prefix() -> TestResult {
    let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], STORES[0], HELPER)?;
    let plan = yaml["jobs"][PLAN].clone();
    let steps = steps(&plan);
    let find = named(&steps, "Find the anchor").ok_or("no find")?;
    assert!(text(find, "uses").starts_with("actions/cache/restore@"));
    assert_eq!(find["with"]["restore-keys"], PREFIX);
    assert_eq!(
        find["with"]["key"],
        format!("{PREFIX}${{{{ github.run_id }}}}-${{{{ github.job }}}}")
    );
    let read = named(&steps, "Read the anchor").ok_or("no read")?;
    assert!(text(read, "run").ends_with("tools-verify read"));
    assert_eq!(
        read["env"]["VELNOR_ANCHOR_MATCHED"],
        "${{ steps.toolsanchor.outputs.cache-matched-key }}"
    );
    let at = |name: &str| position(&steps, |step| step["name"] == name);
    assert!(at("Find the anchor") < at("Read the anchor"));
    Ok(())
}

#[test]
fn a_tools_entry_is_named_by_the_anchor_it_must_equal() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, HELPER)?;
        let (mut restores, mut saves, mut anchors) = (0, 0, 0);
        for (id, job) in jobs(&yaml)? {
            for step in steps(&job) {
                let key = text(&step["with"], "key");
                let uses = text(&step, "uses");
                if key.starts_with(PREFIX) {
                    assert!(
                        key == format!("{PREFIX}${{{{ github.run_id }}}}-${{{{ github.job }}}}"),
                        "{config:?} {id}: {key}"
                    );
                    anchors += 1;
                    continue;
                }
                if !key.starts_with(TOOLS_KEY) {
                    continue;
                }
                if uses.starts_with("actions/cache/save@") {
                    assert_eq!(key, format!("{TOOLS_KEY}{RECORDED}"), "{config:?} {id}");
                    saves += 1;
                } else {
                    assert_eq!(key, format!("{TOOLS_KEY}{PLAN_ANCHOR}"), "{config:?} {id}");
                    assert!(step["with"]["restore-keys"].is_null(), "{config:?} {id}");
                    restores += 1;
                }
            }
        }
        assert!(
            restores >= 2 && saves >= 1 && anchors >= 2,
            "{config:?}: {restores} {saves} {anchors}"
        );
    }
    Ok(())
}

#[test]
fn the_node_store_is_keyed_like_the_network_entries() -> TestResult {
    let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], STORES[0], HELPER)?;
    let (mut restores, mut saves) = (0, 0);
    for (id, job) in jobs(&yaml)? {
        for step in steps(&job) {
            let run = text(&step, "run");
            let key = text(&step["env"], "VELNOR_TOOLS_KEY");
            if run.contains("tools-local restore") || run.contains("tools-local save") {
                if run.contains("tools-local save") && !run.contains("Prime") {
                    let primes = step["name"] == "Prime the node store with the tools";
                    let expected = if primes { PLAN_ANCHOR } else { RECORDED };
                    assert_eq!(key, format!("{TOOLS_KEY}{expected}"), "{id}");
                    saves += 1;
                } else {
                    assert_eq!(key, format!("{TOOLS_KEY}{PLAN_ANCHOR}"), "{id}");
                    restores += 1;
                }
            }
        }
    }
    assert!(restores >= 2 && saves >= 1, "{restores} {saves}");
    Ok(())
}

#[test]
fn a_new_anchor_is_saved_with_the_tools_it_names() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, HELPER)?;
        let mut checked = 0;
        for (id, job) in jobs(&yaml)? {
            let steps = steps(&job);
            let Some(record) = position(&steps, |step| {
                text(step, "run").ends_with("tools-verify record")
            }) else {
                continue;
            };
            assert_eq!(
                steps[record]["env"]["VELNOR_TOOLS_ANCHOR"], PLAN_ANCHOR,
                "{id}"
            );
            assert_eq!(
                steps[record]["env"]["VELNOR_TOOLS_VERIFIED"],
                "${{ steps.toolsverify.outputs.verified }}",
                "{id}"
            );
            let anchor = position(&steps, |step| step["name"] == "Save anchor").ok_or("no save")?;
            assert!(record < anchor, "{id}");
            assert!(
                text(&steps[anchor], "if").contains("steps.toolsrecord.outputs.recorded == 'true'")
            );
            let tools = steps
                .iter()
                .filter(|step| {
                    text(step, "uses").starts_with("actions/cache/save@")
                        && text(&step["with"], "key").starts_with(TOOLS_KEY)
                })
                .collect::<Vec<_>>();
            assert_eq!(tools.len(), 1, "{id}");
            let condition = text(tools[0], "if");
            for needed in [
                "steps.toolsrecord.outputs.matches == 'true'",
                "steps.toolsrecord.outputs.recorded == 'true'",
            ] {
                assert!(condition.contains(needed), "{config:?} {id}: {condition}");
            }
            assert!(
                condition.contains("github.event.repository.default_branch"),
                "{config:?} {id}: {condition}"
            );
            checked += 1;
        }
        assert_ne!(checked, 0, "{config:?}");
    }
    Ok(())
}

#[test]
fn a_job_without_a_plan_job_reads_the_anchor_itself() -> TestResult {
    let yaml = render(&["Cargo.toml"], "shape = \"one-job\"\n", HELPER)?;
    let all = jobs(&yaml)?;
    assert!(
        all.iter().all(|(id, _)| id != PLAN),
        "{:?}",
        all.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );
    let mut checked = 0;
    for (id, job) in all {
        let steps = steps(&job);
        let Some(verify) = position(&steps, verifies) else {
            continue;
        };
        let find = position(&steps, |step| step["name"] == "Find the anchor").ok_or("no find")?;
        let read = position(&steps, |step| step["name"] == "Read the anchor").ok_or("no read")?;
        let restore = position(&steps, restores_tools).ok_or("no restore")?;
        assert!(find < read && read < restore && restore < verify, "{id}");
        assert_eq!(
            steps[verify]["env"]["VELNOR_TOOLS_ANCHOR"], READ_ANCHOR,
            "{id}"
        );
        let key = text(&steps[restore]["with"], "key");
        assert_eq!(key, format!("{TOOLS_KEY}{READ_ANCHOR}"), "{id}");
        checked += 1;
    }
    assert_eq!(checked, 1);
    Ok(())
}

#[test]
fn a_helper_without_the_newest_anchor_keeps_one_anchor_restore_per_job() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, "0.1.109")?;
        let mut restores = 0;
        for (id, job) in jobs(&yaml)? {
            assert!(job["outputs"]["tools_anchor"].is_null(), "{config:?} {id}");
            let steps = steps(&job);
            assert!(
                named(&steps, "Find the anchor").is_none(),
                "{config:?} {id}"
            );
            assert!(
                named(&steps, "Read the anchor").is_none(),
                "{config:?} {id}"
            );
            restores += steps
                .iter()
                .filter(|step| step["name"] == "Restore anchor")
                .count();
        }
        assert!(restores >= 2, "{config:?}");
    }
    Ok(())
}
