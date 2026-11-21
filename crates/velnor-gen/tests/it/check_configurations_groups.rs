// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::check_configurations::found;
use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::view_with;
use crate::yaml_tree::document;

const NEXT: &str = "0.1.112";

#[test]
fn a_grouped_root_runs_the_check_in_the_group_target_directory() -> TestResult {
    let config = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n[[check_configurations]]\nroot = 'tools/a'\n";
    let view = view_with(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        config,
        NEXT,
    )?;
    let steps = found(&document(&workflow(&view)?)?["jobs"]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(
        steps[0].3["env"]["CARGO_TARGET_DIR"], "${{ github.workspace }}/target-velnor/tools",
        "{steps:?}"
    );
    Ok(())
}

fn mixed_split_group(
    testless: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let config = format!("shape = 'lint-test'\n[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n[[check_configurations]]\nroot = '{testless}'\n");
    let mut view = view_with(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        &config,
        helper,
    )?;
    let manifest = RepoPath::new(format!("{testless}/Cargo.toml"))?;
    let metadata = view
        .rust_metadata
        .get_mut(&manifest)
        .ok_or("no metadata for the testless root")?;
    for member in &mut metadata.members {
        for target in &mut member.targets {
            target.tests = false;
        }
    }
    Ok(view)
}

#[test]
fn a_testless_first_member_of_a_mixed_split_group_gets_its_check_in_the_lint_job() -> TestResult {
    let view = mixed_split_group("tools/a", NEXT)?;
    let steps = found(&document(&workflow(&view)?)?["jobs"]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert!(steps[0].0.contains("lint"), "{steps:?}");
    Ok(())
}

#[test]
fn before_0_1_112_a_mixed_split_group_keeps_its_released_rendering_without_the_check() -> TestResult
{
    let view = mixed_split_group("tools/a", "0.1.111")?;
    assert_eq!(found(&document(&workflow(&view)?)?["jobs"]).len(), 0);
    Ok(())
}

#[test]
fn a_testless_grouped_root_beside_a_tested_one_still_gets_its_check() -> TestResult {
    let view = mixed_split_group("tools/b", NEXT)?;
    let steps = found(&document(&workflow(&view)?)?["jobs"]);
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].3["working-directory"], "tools/b", "{steps:?}");
    Ok(())
}

#[test]
fn a_grouped_root_check_runs_before_its_own_tests_not_a_sibling_roots() -> TestResult {
    let config = "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n[[check_configurations]]\nroot = 'tools/b'\n";
    let view = view_with(
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        config,
        NEXT,
    )?;
    let jobs = document(&workflow(&view)?)?["jobs"].clone();
    let steps = found(&jobs);
    assert_eq!(steps.len(), 1, "{steps:?}");
    let (id, index, _, _) = &steps[0];
    let next = jobs[id.as_str()]["steps"][index + 1]["run"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(next.contains("tools/b/Cargo.toml"), "{id}: {next}");
    Ok(())
}
