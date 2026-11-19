// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::non_input_checkout::NON_INPUT_GUARD_STEP;

use super::non_inputs_checkout_support::rust_repo;
use super::scoped_workspace::TestResult;
use super::tools_producers::POLICIES;

#[test]
fn default_non_inputs_is_empty_and_changes_nothing() -> TestResult {
    for policy in POLICIES {
        let omitted = rust_repo(policy, None)?;
        let explicit = rust_repo(policy, None)?;
        let config_path = explicit.path().join(".velnor/config.toml");
        let config = fs::read_to_string(&config_path)?;
        fs::write(
            &config_path,
            config.replacen("[stacks.rust]\n", "[stacks.rust]\nnon_inputs = []\n", 1),
        )?;
        let left = render_staged_tree(&prepare(omitted.path())?)?;
        let right = render_staged_tree(&prepare(explicit.path())?)?;
        assert_eq!(left.files.len(), right.files.len(), "{policy:?}");
        for (a, b) in left.files.iter().zip(&right.files) {
            assert_eq!(a.path, b.path, "{policy:?}");
            assert_eq!(a.bytes, b.bytes, "{policy:?}: {}", a.path);
            assert!(
                !a.bytes.contains("sparse-checkout"),
                "{policy:?}: {}",
                a.path
            );
            assert!(
                !a.bytes.contains(NON_INPUT_GUARD_STEP),
                "{policy:?}: {}",
                a.path
            );
        }
    }
    Ok(())
}
