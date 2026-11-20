// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const SELF_HOSTED: &str = "runner = \"scale-set\"\nhosted = false\n";
const NOT_SELF_HOSTED: &str = "runner.environment != 'self-hosted'";
const NETWORK_STEPS: [&str; 3] = [
    "Restore the tools cache",
    "Look up the tools cache",
    "Save the tools cache",
];

type Steps = Vec<(String, Vec<serde_json::Value>)>;

fn jobs_for(config: &str, helper: &str) -> Result<Steps, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    crate::pipeline::pin_helper(&mut repo, helper)?;
    let yaml: serde_json::Value = crate::yaml_tree::document(&workflow(&repo)?)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
    Ok(jobs
        .iter()
        .map(|(id, job)| {
            (
                id.clone(),
                job["steps"].as_array().cloned().unwrap_or_default(),
            )
        })
        .filter(|(_, steps)| steps.iter().any(|step| step["name"] == "Install tools"))
        .collect())
}

fn at(steps: &[serde_json::Value], name: &str) -> Option<usize> {
    steps.iter().position(|step| step["name"] == name)
}

fn condition(step: &serde_json::Value) -> &str {
    step["if"].as_str().unwrap_or_default()
}

#[test]
fn a_self_hosted_job_restores_the_tools_from_the_node_store_and_skips_the_actions_cache()
-> TestResult {
    for store in [
        "",
        "mbx_shared_store = true\n",
        "mbx_shared_store = false\n",
    ] {
        let jobs = jobs_for(&format!("{SELF_HOSTED}{store}"), "0.1.109")?;
        assert_ne!(jobs.len(), 0, "{store:?}");
        for (id, steps) in &jobs {
            let local = at(steps, "Restore the tools from the node store")
                .ok_or_else(|| format!("{store:?} {id}: no node store restore"))?;
            let verify =
                at(steps, "Verify tools").ok_or_else(|| format!("{store:?} {id}: no verify"))?;
            let install =
                at(steps, "Install tools").ok_or_else(|| format!("{store:?} {id}: no install"))?;
            assert!(local < verify && verify < install, "{store:?} {id}");
            for name in NETWORK_STEPS {
                for step in steps.iter().filter(|step| step["name"] == name) {
                    let when = condition(step);
                    assert!(
                        when.contains(NOT_SELF_HOSTED),
                        "{store:?} {id} {name}: {when}"
                    );
                    let order = steps.iter().position(|other| other == step);
                    assert!(order.is_some_and(|n| n > local), "{store:?} {id} {name}");
                }
            }
            assert!(
                at(steps, "Restore the tools cache").is_some(),
                "{store:?} {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_hosted_job_and_an_older_helper_keep_the_actions_cache_restore_as_before() -> TestResult {
    let cases = [
        ("", "0.1.109"),
        (SELF_HOSTED, "0.1.108"),
        ("mbx_shared_store = true\n", "0.1.109"),
    ];
    for (config, helper) in cases {
        for (id, steps) in jobs_for(config, helper)? {
            for step in &steps {
                assert!(
                    !condition(step).contains(NOT_SELF_HOSTED),
                    "{config:?} {helper} {id}"
                );
            }
            let has_local = at(&steps, "Restore the tools from the node store").is_some();
            assert_eq!(
                has_local,
                config.contains("mbx_shared_store = true"),
                "{config:?} {helper} {id}"
            );
        }
    }
    Ok(())
}
