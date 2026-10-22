//! Release-gate script shape: one pinned `gh` definition, scrubbed linters.

use std::error::Error;

use velnor_actions_contract::RoutingWorkflow;

use super::super::super::product_release_family as family;
use super::super::super::product_release_test_pins::test_pins;
use super::super::Family;
use super::{all_text, product};
/// `gh() {` definitions outside the embedded prepare heredoc, which runs in
/// its own `bash -s` and so defines its own.
fn top_level_gh_definitions(script: &str) -> usize {
    let mut in_prepare = false;
    let mut count = 0;
    for line in script.lines() {
        if line.contains("<<'VELNOR_PREPARE'") {
            in_prepare = true;
        } else if in_prepare && line == "VELNOR_PREPARE" {
            in_prepare = false;
        } else if !in_prepare && line == "gh() {" {
            count += 1;
        }
    }
    count
}

#[test]
fn every_publisher_defines_the_pinned_gh_function_once_before_eligibility()
-> Result<(), Box<dyn Error>> {
    for family in [Family::Images, Family::Binary] {
        let script = family::publish_script(family, &test_pins())?;
        assert_eq!(top_level_gh_definitions(&script), 1, "{script}");
        let gh = script.find("gh() {").ok_or("gh function")?;
        let eligibility = script
            .find("release_eligibility() {")
            .ok_or("eligibility function")?;
        assert!(gh < eligibility, "{script}");
        assert_eq!(script.matches("release_eligibility() {").count(), 1);
    }
    Ok(())
}

#[test]
fn release_gate_linters_run_without_ambient_tokens() -> Result<(), Box<dyn Error>> {
    let product = product(&[
        RoutingWorkflow::GeneratorRelease,
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
    ])?;
    let all = all_text(&product);
    let mut gates = 0;
    for name in ["name: Run jactionlint"] {
        for step in all.split(name).skip(1) {
            let run = step
                .lines()
                .find(|line| line.trim_start().starts_with("run:"))
                .ok_or("lint step run line")?;
            let env_scrub = run.contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN")
                && run.contains("-u GITHUB_TOKEN")
                && run.contains("-u GH_TOKEN");
            let inline_scrub = run.contains("bash -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN")
                && run.contains(" GITHUB_TOKEN ")
                && run.contains(" GH_TOKEN ");
            assert!(env_scrub || inline_scrub, "{name}: {run}");
            gates += 1;
        }
    }
    assert!(
        gates >= 2,
        "expected release-gate lint steps, found {gates}"
    );
    Ok(())
}
