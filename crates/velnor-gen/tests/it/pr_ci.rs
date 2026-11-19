// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value as Json;
use velnor_model::RepoPath;

use crate::base_save::pinned;
use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::pr_ci_sim::{Event, fork_pull, labeled, other, outcome, pull, state, strings};

pub(crate) const LABEL_CONFIG: &str = "pr_ci = \"label\"\nshape = \"lint-test\"\n";
const GATE: &str = "velnor-ci gate";
const LENIENT: &str = "Fail when a Rust job failed or was cancelled";
const NOTICE: &str = "Note that CI is not requested on a draft pull request";
const QUICK_JOBS: [&str; 3] = ["rust-plan", "rust-lint", "policy"];
const FULL_JOBS: [&str; 1] = ["rust-test"];
const RESET: &str = "ci-run-label";

pub(crate) fn render(version: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = pinned(version, config)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
    }
    workflow(&view)
}

pub(crate) fn label_workflow() -> Result<Json, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(&render(
        "0.1.86",
        LABEL_CONFIG,
    )?)?)
}

#[test]
fn an_unset_pr_ci_renders_the_same_workflow_as_always_and_none_of_the_label_machinery() -> TestResult
{
    let unset = render("0.1.86", "shape = \"lint-test\"\n")?;
    let always = render("0.1.86", "shape = \"lint-test\"\npr_ci = \"always\"\n")?;
    assert_eq!(unset, always);
    for word in ["labeled", "ci:run", "ci:auto", RESET, "draft"] {
        assert!(!unset.contains(word), "{word}");
    }
    Ok(())
}

#[test]
fn the_label_mode_starts_on_exactly_the_default_types_plus_draft_conversion_and_labeled_and_keeps_edited_for_a_base_retarget()
-> TestResult {
    let tree = label_workflow()?;
    assert_eq!(
        strings(tree.pointer("/on/pull_request/types")),
        [
            "opened",
            "synchronize",
            "reopened",
            "ready_for_review",
            "edited",
            "converted_to_draft",
            "labeled"
        ]
    );
    Ok(())
}

struct Row {
    what: &'static str,
    event: Event,
    runs: Runs,
    reset: &'static str,
}

#[derive(Clone, Copy)]
enum Runs {
    Nothing,
    Quick,
    Everything,
}

impl Runs {
    fn jobs(self) -> (Vec<&'static str>, Vec<&'static str>) {
        let everything: Vec<&str> = QUICK_JOBS.into_iter().chain(FULL_JOBS).collect();
        match self {
            Self::Nothing => (Vec::new(), everything),
            Self::Quick => (QUICK_JOBS.to_vec(), FULL_JOBS.to_vec()),
            Self::Everything => (everything, Vec::new()),
        }
    }

    fn required_steps(self) -> Vec<&'static str> {
        match self {
            Self::Nothing => vec![NOTICE],
            Self::Quick => Vec::new(),
            Self::Everything => vec![
                "Resolve released helper tools",
                "Install velnor-ci 0.1.86",
                "Install the released helper launcher",
                GATE,
            ],
        }
    }
}

fn rows() -> Vec<Row> {
    let queue = "refs/heads/gh-readonly-queue/main/pr-7";
    let row = |what, event, runs, reset| Row {
        what,
        event,
        runs,
        reset,
    };
    vec![
        row("a draft", pull(true, &[]), Runs::Nothing, "skipped"),
        row(
            "a draft that carries ci:run",
            pull(true, &["ci:run"]),
            Runs::Nothing,
            "skipped",
        ),
        row(
            "a ready pull request without a label",
            pull(false, &["bug"]),
            Runs::Quick,
            "skipped",
        ),
        row(
            "ci:run",
            pull(false, &["ci:run"]),
            Runs::Everything,
            "success",
        ),
        row(
            "ci:auto",
            pull(false, &["ci:auto"]),
            Runs::Everything,
            "skipped",
        ),
        row(
            "ci:run on a fork pull request",
            fork_pull(&["ci:run"]),
            Runs::Quick,
            "skipped",
        ),
        row(
            "ci:auto on a fork pull request",
            fork_pull(&["ci:auto"]),
            Runs::Quick,
            "skipped",
        ),
        row(
            "the ci:run label being added",
            labeled("ci:run", &["ci:run"]),
            Runs::Everything,
            "success",
        ),
        row(
            "the ci:auto label being added",
            labeled("ci:auto", &["ci:auto"]),
            Runs::Everything,
            "skipped",
        ),
        row(
            "a merge group",
            other("merge_group", queue),
            Runs::Everything,
            "skipped",
        ),
        row(
            "a push to main",
            other("push", "refs/heads/main"),
            Runs::Everything,
            "skipped",
        ),
    ]
}

#[test]
fn a_pull_request_runs_what_its_draft_state_and_labels_request_and_main_runs_everything()
-> TestResult {
    let tree = label_workflow()?;
    for row in rows() {
        let what = row.what;
        let (results, verdict) = outcome(&tree, &row.event, &[])?;
        let (ran, skipped) = row.runs.jobs();
        for job in ran {
            assert_eq!(state(&results, job), "success", "{what}: {job}");
        }
        for job in skipped {
            assert_eq!(state(&results, job), "skipped", "{what}: {job}");
        }
        assert_eq!(state(&results, "required"), "success", "{what}");
        assert_eq!(verdict.steps, row.runs.required_steps(), "{what}");
        assert!(verdict.passed, "{what}");
        assert_eq!(state(&results, RESET), row.reset, "{what}");
    }
    Ok(())
}

#[test]
fn a_failure_in_the_quick_set_fails_required_and_the_full_run_leaves_the_verdict_to_the_gate()
-> TestResult {
    let tree = label_workflow()?;
    for failing in [vec!["rust-lint"], vec!["rust-plan"], vec!["policy"]] {
        let (_, verdict) = outcome(&tree, &pull(false, &[]), &failing)?;
        assert!(!verdict.passed, "{failing:?}");
        assert_eq!(verdict.steps, [LENIENT], "{failing:?}");
    }
    let (_, verdict) = outcome(&tree, &pull(false, &["ci:run"]), &["rust-test"])?;
    assert!(verdict.steps.iter().any(|step| step == GATE));
    assert!(!verdict.steps.iter().any(|step| step == LENIENT));
    Ok(())
}

#[test]
fn required_on_a_merge_group_or_a_push_to_main_needs_every_lane_and_allows_no_skip() -> TestResult {
    let label = label_workflow()?;
    let always = crate::yaml_tree::document(&render("0.1.86", "shape = \"lint-test\"\n")?)?;
    let needs = strings(label.pointer("/jobs/required/needs"));
    let expected: Vec<String> = label
        .get("jobs")
        .and_then(Json::as_object)
        .ok_or("no jobs")?
        .keys()
        .filter(|id| !["required", "cache", "rust-trim", RESET].contains(&id.as_str()))
        .cloned()
        .collect();
    let mut sorted = needs.clone();
    sorted.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(sorted, expected);
    assert!(!needs.iter().any(|need| need == RESET));
    assert_eq!(
        label
            .pointer("/jobs/required/steps")
            .and_then(Json::as_array)
            .map(|steps| steps
                .iter()
                .filter_map(|step| step.pointer("/env/GATE_SPEC"))
                .collect::<Vec<_>>()),
        always
            .pointer("/jobs/required/steps")
            .and_then(Json::as_array)
            .map(|steps| steps
                .iter()
                .filter_map(|step| step.pointer("/env/GATE_SPEC"))
                .collect::<Vec<_>>()),
    );
    let events = [
        other("merge_group", "refs/heads/gh-readonly-queue/main/pr-7"),
        other("push", "refs/heads/main"),
    ];
    for event in events {
        let (results, verdict) = outcome(&label, &event, &[])?;
        let (baseline, _) = outcome(&always, &event, &[])?;
        for need in &needs {
            assert_eq!(
                state(&results, need),
                state(&baseline, need),
                "{} {need}",
                event.name
            );
        }
        for job in QUICK_JOBS.into_iter().chain(FULL_JOBS) {
            assert_eq!(state(&results, job), "success", "{} {job}", event.name);
        }
        assert!(verdict.steps.iter().any(|step| step == GATE));
        assert!(!verdict.steps.iter().any(|step| step == LENIENT));
        assert!(!verdict.steps.iter().any(|step| step == NOTICE));
        let (_, failed) = outcome(&label, &event, &["rust-test"])?;
        assert!(failed.steps.iter().any(|step| step == GATE));
        assert!(!failed.steps.iter().any(|step| step == LENIENT));
    }
    Ok(())
}

#[test]
fn only_the_label_removal_job_can_write_pull_requests_and_it_runs_no_pull_request_code()
-> TestResult {
    let tree = label_workflow()?;
    let jobs = tree
        .get("jobs")
        .and_then(Json::as_object)
        .ok_or("no jobs")?;
    for (id, job) in jobs {
        let writes = job
            .pointer("/permissions/pull-requests")
            .and_then(Json::as_str)
            == Some("write");
        assert_eq!(writes, id == RESET, "{id}");
    }
    let reset = jobs.get(RESET).ok_or("no label removal job")?;
    assert_eq!(
        reset.get("permissions"),
        Some(&serde_json::json!({"pull-requests": "write"}))
    );
    assert_eq!(
        strings(reset.get("needs")),
        ["required"],
        "the removal job waits for Required and nothing else"
    );
    let steps = reset
        .get("steps")
        .and_then(Json::as_array)
        .ok_or("no steps")?;
    assert_eq!(steps.len(), 1);
    assert!(steps[0].get("uses").is_none());
    let run = steps[0].get("run").and_then(Json::as_str).ok_or("no run")?;
    assert!(run.starts_with("curl "), "{run}");
    assert!(run.ends_with("/labels/ci%3Arun\""), "{run}");
    Ok(())
}

#[test]
fn pr_ci_label_is_refused_with_skip_draft_an_unknown_value_or_a_helper_that_does_not_know_it() {
    let refused =
        |version: &str, config: &str| render(version, config).err().map(|e| e.to_string());
    let both = refused(
        "0.1.86",
        "pr_ci = \"label\"\nskip_draft_pull_requests = true\n",
    );
    assert!(both.is_some_and(|text| text.contains("skip_draft_pull_requests")));
    let unknown = refused("0.1.86", "pr_ci = \"sometimes\"\n");
    assert!(unknown.is_some_and(|text| text.contains("pr_ci")));
    assert!(render("0.1.86", LABEL_CONFIG).is_ok());
    assert!(render("0.1.81", "shape = \"lint-test\"\n").is_ok());
}

#[test]
fn a_one_job_workflow_lints_a_ready_pull_request_and_tests_only_on_request() -> TestResult {
    let text = render("0.1.86", "pr_ci = \"label\"\nshape = \"one-job\"\n")?;
    let tree = crate::yaml_tree::document(&text)?;
    let steps = tree
        .pointer("/jobs/rust/steps")
        .and_then(Json::as_array)
        .ok_or("no steps")?;
    let gated: Vec<&str> = steps
        .iter()
        .filter(|step| {
            step.get("if")
                .and_then(Json::as_str)
                .is_some_and(|text| text.contains("ci:run"))
        })
        .filter_map(|step| step.get("name").and_then(Json::as_str))
        .collect();
    assert_ne!(gated.len(), 0);
    assert!(gated.iter().all(|name| name.contains("est")), "{gated:?}");
    for name in ["Format", "Lint"] {
        let step = steps
            .iter()
            .find(|step| step.get("name").and_then(Json::as_str) == Some(name))
            .ok_or("a lint step is missing")?;
        assert!(
            !step
                .get("if")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .contains("ci:run"),
            "{name}"
        );
    }
    for (event, job) in [
        (pull(true, &["ci:run"]), "skipped"),
        (pull(false, &[]), "success"),
    ] {
        let (results, verdict) = outcome(&tree, &event, &[])?;
        assert_eq!(state(&results, "rust"), job);
        assert!(verdict.passed);
    }
    Ok(())
}

#[test]
fn no_released_helper_gets_label_mode_because_none_parses_the_pr_ci_key() {
    for released in ["0.1.81", "0.1.82", "0.1.83", "0.1.84", "0.1.85"] {
        let refusal = render(released, LABEL_CONFIG).err().map(|e| e.to_string());
        assert!(
            refusal.is_some_and(|text| text.contains("pin velnor-ci 0.1.86 or later")),
            "velnor-ci {released} must be refused"
        );
        assert!(
            render(released, "shape = \"lint-test\"\n").is_ok(),
            "velnor-ci {released} still renders without pr_ci"
        );
    }
}
