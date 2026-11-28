// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

use velnor_model::RepoPath;

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
fn a_helper_before_0_1_104_runs_a_plain_prepare_everywhere() -> TestResult {
    let workflow = rendered("0.1.103")?;
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
fn from_0_1_104_prepare_names_exactly_the_roots_its_job_reads() -> TestResult {
    let workflow = rendered("0.1.104")?;
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

fn complete_profile_workflow(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view_with(&["Cargo.toml", "lab/Cargo.toml"], GROUPS, helper)?;
    let path = RepoPath::new("rust-toolchain.toml")?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(
        path,
        b"[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\n".to_vec(),
    );
    workflow(&view)
}

#[test]
fn the_complete_profile_is_refused_before_0_1_104_and_accepted_from_it() -> TestResult {
    let error = complete_profile_workflow("0.1.103")
        .err()
        .ok_or("the complete profile was accepted at 0.1.103")?
        .to_string();
    assert!(
        error.contains("toolchain.profile = \"complete\""),
        "{error}"
    );
    assert!(error.contains("0.1.104"), "{error}");
    complete_profile_workflow("0.1.104")?;
    Ok(())
}
