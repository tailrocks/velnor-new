// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::{steps, view_with};
use crate::yaml_tree::document;

const GROUPS: &str =
    "[[test_groups]]\nname = 'shell'\nroot = 'lab'\nfilter = 'all()'\nneeds = ['cargo']\n";
const PREPARE: &str = ". \"$VELNOR_EXEC\" prepare";
const OUTPUT: &str = "steps.prepare.outputs.lock_";

fn rendered(helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let view = view_with(&["Cargo.toml", "lab/Cargo.toml"], GROUPS, helper)?;
    Ok(document(&workflow(&view)?)?)
}

fn read_roots(job: &Value) -> BTreeSet<String> {
    job.to_string()
        .split(OUTPUT)
        .skip(1)
        .map(|tail| {
            tail.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

fn prepare_runs(job: &Value) -> Vec<&str> {
    steps(job)
        .iter()
        .filter(|step| step["name"] == "velnor-ci prepare")
        .filter_map(|step| step["run"].as_str())
        .collect()
}

#[test]
fn a_helper_before_0_1_103_runs_a_plain_prepare_everywhere() -> TestResult {
    let workflow = rendered("0.1.102")?;
    let mut seen = 0;
    for job in workflow["jobs"]
        .as_object()
        .into_iter()
        .flat_map(|jobs| jobs.values())
    {
        for run in prepare_runs(job) {
            assert_eq!(run, PREPARE);
            seen += 1;
        }
    }
    assert!(seen > 0);
    Ok(())
}

#[test]
fn from_0_1_103_prepare_names_exactly_the_roots_its_job_reads() -> TestResult {
    let workflow = rendered("0.1.103")?;
    let (mut single, mut none) = (0, 0);
    for (id, job) in workflow["jobs"].as_object().into_iter().flatten() {
        let read = read_roots(job);
        for run in prepare_runs(job) {
            let expected = if read.is_empty() {
                none += 1;
                format!("{PREPARE} --no-locks")
            } else {
                single += usize::from(read.len() == 1);
                let flags: String = read
                    .iter()
                    .flat_map(|root| [" --lock-root ", root.as_str()])
                    .collect();
                format!("{PREPARE}{flags}")
            };
            assert_eq!(run, expected, "{id}");
        }
    }
    assert!(single >= 2, "a lane job reads only its own root");
    assert!(none >= 1, "a test group job reads no root");
    Ok(())
}
