// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::newest_helper::newest_helper;
use crate::pipeline::{pin_helper, view, workflow};
use crate::yaml_tree::{Node, parse};

const RESTORED: &str = "\"$VELNOR_EXEC\" restored --target-dir";

fn twenty_roots(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let manifests: Vec<String> = (0..20)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect();
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&roots, None)?;
    pin_helper(&mut repo, helper)?;
    workflow(&repo)
}

fn restored_runs(text: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let tree = parse(text)?;
    let jobs = tree.get("jobs").ok_or("no jobs")?;
    let mut runs = Vec::new();
    for job in jobs.keys() {
        for step in jobs
            .at(&[job, "steps"])
            .map(Node::items)
            .unwrap_or_default()
        {
            if let Some(run) = step.get("run").and_then(|run| run.text())
                && run.contains(RESTORED)
            {
                runs.push(run.to_owned());
            }
        }
    }
    Ok(runs)
}

#[test]
fn a_command_repeated_in_every_root_job_is_written_once_for_the_newest_helper() -> TestResult {
    let text = twenty_roots(&newest_helper()?)?;
    assert_eq!(text.matches(RESTORED).count(), 1, "written once");
    let runs = restored_runs(&text)?;
    assert!(runs.len() >= 20, "{} jobs", runs.len());
    assert!(
        runs.iter().all(|run| run == &runs[0]),
        "every job reads the same command"
    );
    assert!(runs[0].contains("/crate_with_a_long_directory_name_19"));
    Ok(())
}

#[test]
fn an_older_helper_writes_the_command_in_every_root_job() -> TestResult {
    let text = twenty_roots("0.1.110")?;
    let runs = restored_runs(&text)?;
    assert_eq!(text.matches(RESTORED).count(), runs.len());
    assert!(runs.len() >= 20, "{} jobs", runs.len());
    assert!(!text.contains("run: *velnor_"), "no run is shared");
    Ok(())
}
