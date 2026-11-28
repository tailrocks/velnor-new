// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::RepoPath;

use super::reuse_verdict::{KNOB, step};
use crate::facts::TestResult;
use crate::lint_lane::{LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};

const FILTER: &str = "test(/ --no-tests=x/)";
const GROUP: &str = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n";

fn jobs_of(roots: &[&str], extra: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let config = format!("{KNOB}{extra}{LINT}");
    let mut repo = view(roots, Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, "0.1.117")?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone())
}

#[test]
fn a_filter_that_spells_a_flag_stays_whole_in_the_build_only_twin() -> TestResult {
    let lane = format!(
        "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = '{FILTER}'\njob = 'parallel'\n"
    );
    let jobs = jobs_of(&["Cargo.toml", "lab/Cargo.toml"], &lane)?;
    let job = &jobs["rust-lane-local-mode-root"];
    let twin = step(job, "Test (features: local-mode) (build only)")?;
    let run = twin["run"].as_str().unwrap_or_default();
    assert!(run.contains(FILTER), "{run}");
    assert!(run.ends_with(" --no-run"), "{run}");
    assert_eq!(run.matches('"').count() % 2, 0, "{run}");
    assert!(!run.contains("--no-tests=fail"), "{run}");
    assert!(
        step(job, "Test (features: local-mode)")?["run"]
            .as_str()
            .is_some_and(|run| run.contains(FILTER) && run.contains("--no-tests=fail"))
    );
    Ok(())
}

#[test]
fn grouped_roots_name_their_reusable_steps_by_root_and_generate_with_reuse_on() -> TestResult {
    let jobs = jobs_of(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        GROUP,
    )?;
    let (job, _) = jobs
        .as_object()
        .into_iter()
        .flatten()
        .find(|(_, job)| step(job, "Format (tools/a)").is_ok())
        .ok_or("no job names its format step by root")?;
    let job = &jobs[job.as_str()];
    for name in ["Format (tools/a)", "Format (tools/b)"] {
        step(job, name)?;
    }
    assert!(step(job, "Format").is_err());
    let list = step(&jobs["rust-plan"], "velnor-ci verdict reuse")?["env"]["VELNOR_REUSABLE"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    for name in ["Format (tools/a)", "Format (tools/b)"] {
        assert!(list.contains(&format!("\"step\":\"{name}\"")), "{list}");
    }
    Ok(())
}
