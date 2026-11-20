// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{
    RenderedJob, RenderedStep, RenderedTrusted, RenderedWorkflow, render_workflow,
};

use crate::facts::TestResult;
use crate::rules::plan;
use crate::trusted_events::{FORK_RUNNER, NORMAL, TRUSTED, events, runs_on};

const FENCE: &str = "(!startsWith(github.event_name, 'pull_request') || github.event.pull_request.head.repo.full_name == github.repository)";

fn rendered(job: RenderedJob) -> Result<String, Box<dyn std::error::Error>> {
    let workflow = RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "build".to_owned(),
            timeout_minutes: 10,
            steps: vec![RenderedStep {
                name: "step".to_owned(),
                run: Some("echo ok".to_owned()),
                ..RenderedStep::default()
            }],
            ..job
        }],
    };
    let files = render_workflow(&plan(Vec::new())?, &workflow).map_err(|v| format!("{v:?}"))?;
    let text = files.values().next().ok_or("workflow missing")?;
    let yaml: serde_json::Value = crate::yaml_tree::document(text)?;
    Ok(yaml["jobs"]["build"]["runs-on"]
        .as_str()
        .ok_or("no runs-on")?
        .to_owned())
}

fn trusted(label: &str, replaces: &str) -> RenderedTrusted {
    RenderedTrusted {
        label: label.to_owned(),
        replaces: replaces.to_owned(),
    }
}

fn route(expression: &str, matrix: &[(&str, &str)], normal: &str, fork: &str) -> TestResult {
    for (event, outcome) in events() {
        let want = match outcome {
            TRUSTED => "T",
            NORMAL => normal,
            FORK_RUNNER => fork,
            other => return Err(other.into()),
        };
        assert_eq!(
            runs_on(expression, &event, matrix)?,
            want,
            "{} {}: {expression}",
            event.name,
            event.git_ref
        );
    }
    Ok(())
}

#[test]
fn a_fenced_label_routes_to_the_trusted_label_on_default_branch_events() -> TestResult {
    let expression = rendered(RenderedJob {
        runs_on: "R".to_owned(),
        fork_runner: Some("F".to_owned()),
        trusted_runner: Some(trusted("T", "R")),
        ..RenderedJob::default()
    })?;
    assert!(
        expression.contains(&format!("|| ({FENCE} && 'R' || 'F')")),
        "{expression}"
    );
    route(&expression, &[], "R", "F")
}

#[test]
fn a_plain_label_routes_to_the_trusted_label_on_default_branch_events() -> TestResult {
    let expression = rendered(RenderedJob {
        runs_on: "R".to_owned(),
        trusted_runner: Some(trusted("T", "R")),
        ..RenderedJob::default()
    })?;
    assert!(
        expression.ends_with("&& 'T' || 'R' }}") || expression.ends_with("&& 'T' || 'R'"),
        "{expression}"
    );
    for (event, outcome) in events() {
        let want = if outcome == TRUSTED { "T" } else { "R" };
        assert_eq!(runs_on(&expression, &event, &[])?, want, "{}", event.name);
    }
    Ok(())
}

#[test]
fn an_axis_swaps_only_the_value_that_names_the_replaced_label() -> TestResult {
    let fenced = rendered(RenderedJob {
        runs_on: "R".to_owned(),
        runs_on_axis: Some("os".to_owned()),
        fork_runner: Some("F".to_owned()),
        trusted_runner: Some(trusted("T", "R")),
        ..RenderedJob::default()
    })?;
    route(&fenced, &[("os", "R")], "R", "F")?;
    for (event, outcome) in events() {
        let hosted = runs_on(&fenced, &event, &[("os", "macos")])?;
        let want = if outcome == FORK_RUNNER { "F" } else { "macos" };
        assert_eq!(hosted, want, "{}: {fenced}", event.name);
    }
    let bare = rendered(RenderedJob {
        runs_on: "R".to_owned(),
        runs_on_axis: Some("os".to_owned()),
        trusted_runner: Some(trusted("T", "R")),
        ..RenderedJob::default()
    })?;
    for (event, outcome) in events() {
        let swapped = runs_on(&bare, &event, &[("os", "R")])?;
        assert_eq!(
            swapped,
            if outcome == TRUSTED { "T" } else { "R" },
            "{}",
            event.name
        );
        assert_eq!(
            runs_on(&bare, &event, &[("os", "macos")])?,
            "macos",
            "{}",
            event.name
        );
    }
    Ok(())
}

#[test]
fn without_a_trusted_runner_every_form_renders_as_before() -> TestResult {
    let plain = |job: RenderedJob| {
        rendered(RenderedJob {
            runs_on: "R".to_owned(),
            ..job
        })
    };
    assert_eq!(plain(RenderedJob::default())?, "R");
    assert_eq!(
        plain(RenderedJob {
            fork_runner: Some("F".to_owned()),
            ..RenderedJob::default()
        })?,
        format!("${{{{ {FENCE} && 'R' || 'F' }}}}")
    );
    assert_eq!(
        plain(RenderedJob {
            runs_on_axis: Some("os".to_owned()),
            ..RenderedJob::default()
        })?,
        "${{ matrix.os }}"
    );
    assert_eq!(
        plain(RenderedJob {
            runs_on_axis: Some("os".to_owned()),
            fork_runner: Some("F".to_owned()),
            ..RenderedJob::default()
        })?,
        format!("${{{{ {FENCE} && matrix.os || 'F' }}}}")
    );
    Ok(())
}
