// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::tools_anchor::{
    HELPER, PLAN, READ_ANCHOR, STORES, jobs, position, render, steps, verifies,
};

const PLAN_ANCHOR_KEY: &str = "${{ needs.rust-plan.outputs.tools_anchor_key }}";
const PLAN_ANCHOR_KEY_OUTPUT: &str = "${{ steps.toolsanchor.outputs.cache-matched-key }}";

#[test]
fn a_job_that_names_the_anchor_entry_reads_its_key_from_the_plan_job() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, "0.1.111")?;
        let mut verified = 0;
        for (id, job) in jobs(&yaml)? {
            if id == PLAN {
                assert_eq!(
                    job["outputs"]["tools_anchor_key"], PLAN_ANCHOR_KEY_OUTPUT,
                    "{config:?}"
                );
                continue;
            }
            let steps = steps(&job);
            let Some(verify) = position(&steps, verifies) else {
                continue;
            };
            verified += 1;
            assert_eq!(
                steps[verify]["env"]["VELNOR_ANCHOR_CACHE_KEY"], PLAN_ANCHOR_KEY,
                "{config:?} {id}"
            );
        }
        assert!(verified >= 1, "{config:?}");
    }
    Ok(())
}

#[test]
fn every_declared_anchor_key_output_is_read_and_every_read_one_is_declared() -> TestResult {
    let cases: [(&str, &[&str], &str, bool); 4] = [
        ("0.1.111", &["Cargo.toml", "tools/Cargo.toml"], "", true),
        ("0.1.115", &["Cargo.toml", "tools/Cargo.toml"], "", true),
        ("0.1.116", &["Cargo.toml", "tools/Cargo.toml"], "", false),
        ("0.1.116", &["Cargo.toml"], "shape = \"one-job\"\n", false),
    ];
    for config in STORES {
        for (helper, manifests, shape, keyed) in cases {
            let yaml = render(manifests, &format!("{config}{shape}"), helper)?;
            let all = jobs(&yaml)?;
            let steps_text: Vec<String> = all
                .iter()
                .map(|(_, job)| serde_json::to_string(&job["steps"]).unwrap_or_default())
                .collect();
            let mut declared = 0;
            for (producer, job) in &all {
                let name = format!("needs.{producer}.outputs.tools_anchor_key");
                let is_declared = job["outputs"]
                    .as_object()
                    .is_some_and(|outputs| outputs.contains_key("tools_anchor_key"));
                let readers: Vec<_> = all
                    .iter()
                    .zip(&steps_text)
                    .filter(|(_, steps)| steps.contains(&name))
                    .map(|(reader, _)| reader)
                    .collect();
                for (id, reader) in &readers {
                    assert!(is_declared, "{helper} {id} reads undeclared {name}");
                    let needs = serde_json::to_string(&reader["needs"])?;
                    assert!(needs.contains(producer.as_str()), "{helper} {id} {needs}");
                }
                if helper != "0.1.111" {
                    assert_eq!(
                        is_declared,
                        !readers.is_empty(),
                        "{config:?} {manifests:?} {shape} {helper} {producer}"
                    );
                }
                declared += usize::from(is_declared);
            }
            assert_eq!(declared > 0, keyed, "{config:?} {manifests:?} {helper}");
        }
    }
    Ok(())
}

#[test]
fn the_plan_job_of_helper_0_1_110_emits_the_anchor_digest_and_key_outputs() -> TestResult {
    for config in STORES.iter().copied() {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config, HELPER)?;
        let outputs = yaml["jobs"][PLAN]["outputs"]
            .as_object()
            .ok_or("plan outputs missing")?;
        assert_eq!(outputs["tools_anchor"], READ_ANCHOR, "{config:?}");
        assert_eq!(
            outputs["tools_anchor_key"], PLAN_ANCHOR_KEY_OUTPUT,
            "{config:?}"
        );
    }
    Ok(())
}

#[test]
fn the_anchor_key_output_is_kept_before_helper_0_1_116_and_dropped_from_it() -> TestResult {
    for config in STORES.iter().copied() {
        for (helper, keyed) in [("0.1.111", true), ("0.1.115", true), ("0.1.116", false)] {
            let mut readers = 0;
            for (id, job) in jobs(&render(&["Cargo.toml"], config, helper)?)? {
                let Some(outputs) = job["outputs"].as_object() else {
                    continue;
                };
                if outputs.contains_key("tools_anchor") {
                    readers += 1;
                    let has_key = outputs.contains_key("tools_anchor_key");
                    assert_eq!(has_key, keyed, "{helper} {id}");
                }
            }
            assert!(readers > 0, "{config:?} {helper}");
        }
    }
    Ok(())
}
