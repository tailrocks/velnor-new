// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! pull_request/push Rust suites have no emitter in this engine any more, so
//! the configuration must refuse them instead of dropping them silently.

use std::error::Error;

use velnor_actions_contract::{RustSuiteConfig, RustSuiteTrigger, validate_rust_suites};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::tools_producers_donors::SCHEDULED_SUITES_CONFIG;
use super::workflow_configuration_fixture;

const FILE: &str = ".velnor/config.toml";
const MOVED: &str = "moved to velnor-gen's rust-ci.yml; declare them in .velnor/rust-ci.toml";

fn suite(triggers: &[RustSuiteTrigger], required: bool) -> RustSuiteConfig {
    let mut suite = RustSuiteConfig::new("proof", "default");
    suite.triggers = triggers.to_vec();
    suite.required = required;
    if triggers.contains(&RustSuiteTrigger::Schedule) {
        suite.schedule = vec!["0 4 * * *".to_owned()];
    }
    suite
}

fn refusal(suite: RustSuiteConfig) -> String {
    validate_rust_suites(&[suite], FILE)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default()
}

#[test]
fn a_pull_request_or_push_suite_is_refused_with_the_replacement_named() {
    use RustSuiteTrigger::{PullRequest, Push, WorkflowDispatch};
    for triggers in [
        vec![PullRequest],
        vec![Push],
        vec![PullRequest, Push],
        vec![Push, WorkflowDispatch],
    ] {
        let message = refusal(suite(&triggers, false));
        assert!(message.contains(MOVED), "{triggers:?}: {message}");
    }
    let omitted = RustSuiteConfig::new("proof", "default");
    assert!(refusal(omitted).contains(MOVED), "omitted triggers");
}

#[test]
fn a_required_suite_is_refused_whatever_its_triggers() {
    use RustSuiteTrigger::{PullRequest, Push, Schedule, WorkflowDispatch};
    for triggers in [
        vec![PullRequest, Push],
        vec![WorkflowDispatch],
        vec![Schedule],
    ] {
        let message = refusal(suite(&triggers, true));
        assert!(message.contains(MOVED), "{triggers:?}: {message}");
    }
}

#[test]
fn dispatch_and_schedule_suites_stay_valid() {
    use RustSuiteTrigger::{Schedule, WorkflowDispatch};
    for triggers in [
        vec![WorkflowDispatch],
        vec![Schedule],
        vec![Schedule, WorkflowDispatch],
    ] {
        assert_eq!(refusal(suite(&triggers, false)), "", "{triggers:?}");
    }
}

#[test]
fn generation_refuses_a_required_pull_request_push_suite_and_renders_the_rest()
-> Result<(), Box<dyn Error>> {
    let base = SCHEDULED_SUITES_CONFIG;
    let moved = base.replace(
        "triggers = [\"schedule\"]\n",
        "triggers = [\"pull_request\", \"push\"]\nrequired = true\n",
    );
    assert_ne!(moved, base);
    let repo = workflow_configuration_fixture::make_repo(&moved)?;
    let error = prepare(repo.path())
        .map_err(|error| error.to_string())
        .and_then(|prepared| {
            render_staged_tree(&prepared)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .err()
        .ok_or("a pull_request/push suite generated without an error")?;
    assert!(error.contains(MOVED), "{error}");

    let dispatch = base.replace(
        "triggers = [\"schedule\"]\nschedule = [\"0 4 * * *\"]\n",
        "triggers = [\"workflow_dispatch\", \"schedule\"]\nschedule = [\"0 4 * * *\"]\n",
    );
    let repo = workflow_configuration_fixture::make_repo(&dispatch)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let suites = tree
        .get(".github/workflows/suites.yml")
        .ok_or("dispatch and schedule suites no longer render")?;
    assert!(suites.contains("workflow_dispatch:") && suites.contains("schedule:"));
    Ok(())
}
