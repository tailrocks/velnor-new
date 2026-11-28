// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::RepoPath;

use super::reuse_verdict::KNOB;
use crate::facts::TestResult;
use crate::lint_lane::{LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\njob = 'parallel'\n";

fn outputs(knob: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let config = format!("{}{LANE}{LINT}", if knob { KNOB } else { "" });
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    pin_helper(&mut repo, "0.1.117")?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?["jobs"]["rust-lane-local-mode-root"]["outputs"].clone())
}

#[test]
fn a_lane_that_reuse_can_skip_reports_absent_counts_only_when_reuse_skipped_it() -> TestResult {
    for (name, fallback) in [
        ("tests_run", "0"),
        ("tests_listed", "0"),
        ("tests_by_package", "{}"),
    ] {
        let reused = outputs(true)?[name].as_str().unwrap_or_default().to_owned();
        let expected = format!(
            "${{{{ steps.test.outputs.{name} || (!contains(format(' {{0}} ', needs.rust-plan.outputs.reused), ' rust-lane-local-mode-root.test-1 ') && '{fallback}' || '') }}}}"
        );
        assert_eq!(reused, expected, "{name}");
        let plain = outputs(false)?[name]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert_eq!(
            plain,
            format!("${{{{ steps.test.outputs.{name} || '{fallback}' }}}}"),
            "{name}"
        );
    }
    Ok(())
}
