// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const CLEAR: &str = "tools-local clear";
const STORES: &[&str] = &["mbx_shared_store = true\n", "mbx_shared_store = false\n"];

fn rendered(config: &str, helper: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", helper).into_bytes());
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?)
}

fn run(step: &serde_json::Value) -> &str {
    step["run"].as_str().unwrap_or_default()
}

fn restores_tools(step: &serde_json::Value) -> bool {
    step["name"] == "Restore the tools cache" || run(step).contains("tools-local restore")
}

#[test]
fn the_tool_directories_are_cleared_before_any_tools_restore_from_0_1_110() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = rendered(config, "0.1.110")?;
        let mut checked = 0;
        for (id, job) in yaml["jobs"].as_object().ok_or("jobs missing")? {
            let steps = job["steps"].as_array().cloned().unwrap_or_default();
            let Some(first) = steps.iter().position(restores_tools) else {
                continue;
            };
            let clears: Vec<usize> = (0..steps.len())
                .filter(|index| run(&steps[*index]).ends_with(CLEAR))
                .collect();
            assert_eq!(clears.len(), 1, "{config:?} {id}");
            assert!(clears[0] < first, "{config:?} {id}");
            assert!(steps[clears[0]].get("if").is_none(), "{config:?} {id}");
            assert!(
                steps[clears[0]]["name"] == "Clear the tools directories",
                "{config:?} {id}"
            );
            checked += 1;
        }
        assert_ne!(checked, 0, "{config:?}");
    }
    Ok(())
}

#[test]
fn an_older_helper_renders_no_clear_step() -> TestResult {
    for config in STORES.iter().copied() {
        for helper in ["0.1.107", "0.1.109"] {
            let yaml = rendered(config, helper)?;
            for (id, job) in yaml["jobs"].as_object().ok_or("jobs missing")? {
                for step in job["steps"].as_array().cloned().unwrap_or_default() {
                    assert!(
                        !run(&step).contains("tools-local clear"),
                        "{config:?} {helper} {id}"
                    );
                }
            }
        }
    }
    Ok(())
}
