// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const FIXTURE: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

#[test]
fn a_helper_before_0_1_112_renders_a_lane_guard_as_it_did_before() -> TestResult {
    let config = format!(
        "{FIXTURE}[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\n\
         packages = ['package-0']\nfilter = 'test(~local::)'\n"
    );
    let mut repo = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
    crate::pipeline::pin_helper(&mut repo, "0.1.111")?;
    let jobs = crate::yaml_tree::document(&workflow(&repo)?)?["jobs"].clone();
    let guards: Vec<_> = jobs
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, job)| job["steps"].as_array().map_or_default(Vec::as_slice))
        .filter(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("lane-guard"))
        })
        .collect();
    assert!(!guards.is_empty(), "the 0.1.111 pin renders no lane guard");
    for guard in guards {
        assert_eq!(
            guard["if"].as_str(),
            Some(
                "${{ !cancelled() && steps.restored.outcome == 'success' && (contains(format(' {0} ', (steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)), ' -p package-0 ') || contains(format(' {0} ', (steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)), ' --workspace ')) }}"
            ),
            "a 0.1.111 pin renders the lane guard as it did before 0.1.112: {guard}"
        );
    }
    Ok(())
}
