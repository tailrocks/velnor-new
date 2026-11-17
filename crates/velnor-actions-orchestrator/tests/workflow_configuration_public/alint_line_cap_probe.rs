// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use serde_json::Value;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::non_inputs_checkout_support::rust_repo;
use super::scoped_workspace::TestResult;
use super::tools_producers::POLICIES;

const PROBE_SCRIPT: &str = "scripts/alint-line-cap-probe.sh";
const PROBE_GUARD: &str = "hashFiles('scripts/alint-line-cap-probe.sh') != ''";

#[test]
fn alint_line_cap_probe_is_run_by_bash_which_the_script_requires() -> TestResult {
    for policy in POLICIES {
        let repo = rust_repo(policy, None)?;
        let config_path = repo.path().join(".velnor/config.toml");
        let mut config = fs::read_to_string(&config_path)?;
        config.push_str("\n[workflow.verify]\njobs = [\"alint\"]\n");
        fs::write(&config_path, config)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let ci = tree
            .get(".github/workflows/ci.yml")
            .ok_or("generated ci workflow is missing")?;
        let probe_lines: Vec<&str> = ci
            .lines()
            .filter(|line| line.contains(&format!("bash {PROBE_SCRIPT}")))
            .collect();
        assert_eq!(probe_lines.len(), 1, "{policy:?}: {probe_lines:?}");
        assert!(
            probe_lines[0].ends_with(&format!(" bash {PROBE_SCRIPT}")),
            "{policy:?}: {}",
            probe_lines[0]
        );
    }
    Ok(())
}

#[test]
fn alint_line_cap_probe_step_is_guarded_where_a_consumer_lacks_the_script() -> TestResult {
    for policy in POLICIES {
        let repo = rust_repo(policy, None)?;
        assert!(!repo.path().join(PROBE_SCRIPT).exists());
        let config_path = repo.path().join(".velnor/config.toml");
        let mut config = fs::read_to_string(&config_path)?;
        config.push_str("\n[workflow.verify]\njobs = [\"alint\"]\n");
        fs::write(&config_path, config)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let ci = tree
            .get(".github/workflows/ci.yml")
            .ok_or("generated ci workflow is missing")?;
        let workflow: Value = serde_saphyr::from_str(ci)?;
        let steps = workflow["jobs"]["alint"]["steps"]
            .as_array()
            .ok_or("alint job has no steps")?;
        let probe_steps: Vec<&Value> = steps
            .iter()
            .filter(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains(PROBE_SCRIPT))
            })
            .collect();
        for step in probe_steps {
            assert_eq!(
                step["if"].as_str(),
                Some(PROBE_GUARD),
                "{policy:?}: probe step runs in a consumer without the script: {step}"
            );
        }
    }
    Ok(())
}
