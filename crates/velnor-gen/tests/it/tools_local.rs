// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const RESTORE: &str = "\"$VELNOR_EXEC\" tools-local restore --key \"$VELNOR_TOOLS_KEY\"";
const SAVE: &str = "\"$VELNOR_EXEC\" tools-local save --key \"$VELNOR_TOOLS_KEY\"";
const UNLESS_LOCAL: &str = "${{ steps.toolslocal.outputs.hit != 'true' }}";
const SAVE_UNLESS_LOCAL: &str = "${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && steps.tools.outputs.cache-hit != 'true' && steps.toolslookup.outputs.cache-hit != 'true' }}";
const ON_DEFAULT: &str =
    "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)";
const LOOKUP_ON_NODE_HIT: &str = "${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && steps.toolslocal.outputs.hit == 'true' }}";
const CACHE_RESTORE_PIN: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

fn workflow_for(config: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    workflow(&repo)
}

fn steps_of(text: &str) -> Result<Vec<(String, serde_json::Value)>, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = crate::yaml_tree::document(text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let mut all = Vec::new();
    for (id, job) in jobs {
        for step in job["steps"].as_array().into_iter().flatten() {
            all.push((id.clone(), step.clone()));
        }
    }
    Ok(all)
}

#[test]
fn a_lane_tries_the_node_store_before_the_network_cache_and_saves_it_first() -> TestResult {
    let text = workflow_for("mbx_shared_store = true\n", "0.1.89")?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let mut lanes = 0;
    for (id, job) in jobs {
        let steps = job["steps"].as_array().ok_or("steps missing")?;
        let name = |wanted: &str| steps.iter().position(|s| s["name"] == wanted);
        let Some(network) = name("Restore the tools cache") else {
            continue;
        };
        lanes += 1;
        let local = name("Restore the tools from the node store")
            .ok_or_else(|| format!("{id}: no local restore"))?;
        assert_eq!(local + 1, network, "{id}");
        assert_eq!(steps[local]["id"], "toolslocal", "{id}");
        assert!(
            steps[local]["run"]
                .as_str()
                .is_some_and(|run| run.ends_with(RESTORE)),
            "{id}"
        );
        assert_eq!(steps[network]["if"], UNLESS_LOCAL, "{id}");
        assert!(steps[local].get("if").is_none(), "{id}");
        if let Some(save) = name("Save the tools to the node store") {
            assert!(
                steps[save]["run"]
                    .as_str()
                    .is_some_and(|run| run.ends_with(SAVE)),
                "{id}"
            );
            assert_eq!(steps[save + 1]["name"], "Save the tools cache", "{id}");
            assert_eq!(steps[save + 1]["if"], SAVE_UNLESS_LOCAL, "{id}");
        }
    }
    assert!(lanes > 0);
    assert!(
        steps_of(&text)?
            .iter()
            .any(|(_, s)| s["run"].as_str().is_some_and(|run| run.ends_with(SAVE)))
    );
    Ok(())
}

#[test]
fn the_node_store_needs_the_shared_store_and_helper_0_1_89() -> TestResult {
    for (config, helper) in [
        ("mbx_shared_store = true\n", "0.1.88"),
        ("mbx_shared_store = true\n", "0.1.86"),
        ("", "0.1.89"),
        ("mbx_shared_store = false\n", "0.1.89"),
    ] {
        let text = workflow_for(config, helper)?;
        assert!(!text.contains("tools-local"), "{config:?} {helper}");
        assert!(!text.contains("toolslocal"), "{config:?} {helper}");
    }
    Ok(())
}

type Lane = (String, Vec<serde_json::Value>);

fn lanes_with_tools(text: &str) -> Result<Vec<Lane>, Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = crate::yaml_tree::document(text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    let mut lanes = Vec::new();
    for (id, job) in jobs {
        let steps = job["steps"].as_array().ok_or("steps missing")?.clone();
        if steps.iter().any(|s| s["name"] == "Save the tools cache") {
            lanes.push((id.clone(), steps));
        }
    }
    Ok(lanes)
}

#[test]
fn a_node_hit_on_the_default_branch_looks_up_the_network_entry_and_saves_only_on_a_miss()
-> TestResult {
    let text = workflow_for("mbx_shared_store = true\n", "0.1.89")?;
    let lanes = lanes_with_tools(&text)?;
    assert_ne!(lanes.len(), 0);
    for (id, steps) in &lanes {
        let at = |wanted: &str| steps.iter().position(|s| s["name"] == wanted);
        let lookup = at("Look up the tools cache").ok_or_else(|| format!("{id}: no lookup"))?;
        let save = at("Save the tools cache").ok_or_else(|| format!("{id}: no save"))?;
        let step = &steps[lookup];
        assert_eq!(step["id"], "toolslookup", "{id}");
        assert_eq!(step["uses"], CACHE_RESTORE_PIN, "{id}");
        assert_eq!(
            step["with"]["lookup-only"].to_string().trim_matches('"'),
            "true",
            "{id}"
        );
        assert_eq!(step["with"]["key"], steps[save]["with"]["key"], "{id}");
        assert_eq!(step["if"], LOOKUP_ON_NODE_HIT, "{id}");
        assert!(lookup < save, "{id}");
        assert_eq!(steps[save]["if"], SAVE_UNLESS_LOCAL, "{id}");
    }
    Ok(())
}

#[test]
fn a_node_hit_on_a_pull_request_neither_looks_up_nor_saves_the_tools() -> TestResult {
    let text = workflow_for("mbx_shared_store = true\n", "0.1.89")?;
    let lanes = lanes_with_tools(&text)?;
    assert_ne!(lanes.len(), 0);
    for (id, steps) in &lanes {
        let at = |wanted: &str| steps.iter().position(|s| s["name"] == wanted);
        let lookup = at("Look up the tools cache").ok_or_else(|| format!("{id}: no lookup"))?;
        let save = at("Save the tools cache").ok_or_else(|| format!("{id}: no save"))?;
        for step in [&steps[lookup], &steps[save]] {
            let condition = step["if"].as_str().ok_or_else(|| format!("{id}: no if"))?;
            assert!(condition.contains(ON_DEFAULT), "{id}: {condition}");
            assert!(!condition.contains("pull_request"), "{id}: {condition}");
        }
    }
    Ok(())
}

#[test]
fn a_node_miss_on_the_default_branch_restores_and_saves_the_network_entry_as_before() -> TestResult
{
    let text = workflow_for("mbx_shared_store = true\n", "0.1.89")?;
    let lanes = lanes_with_tools(&text)?;
    assert_ne!(lanes.len(), 0);
    for (id, steps) in &lanes {
        let at = |wanted: &str| steps.iter().position(|s| s["name"] == wanted);
        let network = at("Restore the tools cache").ok_or_else(|| format!("{id}: no restore"))?;
        assert_eq!(steps[network]["if"], UNLESS_LOCAL, "{id}");
        let lookup = at("Look up the tools cache").ok_or_else(|| format!("{id}: no lookup"))?;
        assert_eq!(steps[lookup]["if"], LOOKUP_ON_NODE_HIT, "{id}");
        let save = at("Save the tools cache").ok_or_else(|| format!("{id}: no save"))?;
        assert!(
            steps[save]["if"]
                .as_str()
                .is_some_and(|c| c.contains("steps.tools.outputs.cache-hit != 'true'")),
            "{id}"
        );
    }
    Ok(())
}
