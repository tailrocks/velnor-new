// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{build_only, qualify_shared, with_twin};
use crate::rules::{RenderedJob, RenderedReuse, RenderedStep, RenderedWorkflow, Rule, check_rules};
use velnor_model::{
    CargoCommand, FilterRef, JobId, LaneFeatures, PrRun, Reuse, SetupOrder, Step, StepKind, When,
};

fn reusable(name: &str, root: &str, build_only: bool) -> RenderedStep {
    RenderedStep {
        name: name.to_owned(),
        run: Some("true".to_owned()),
        reuse: Some(RenderedReuse {
            id: format!("{root}.{name}"),
            plan: "plan".to_owned(),
            build_only,
            root: root.to_owned(),
        }),
        ..RenderedStep::default()
    }
}

fn user(name: &str) -> RenderedStep {
    RenderedStep {
        name: name.to_owned(),
        run: Some("true".to_owned()),
        ..RenderedStep::default()
    }
}

fn names(steps: &[RenderedStep]) -> Vec<&str> {
    steps.iter().map(|step| step.name.as_str()).collect()
}

#[test]
fn qualify_shared_roots_the_names_two_reusable_steps_share_and_their_twins() {
    let mut steps = vec![
        reusable("Format", "tools/a", false),
        reusable("Test (build only)", "tools/a", true),
        reusable("Test", "tools/a", false),
        reusable("Format", "tools/b", false),
        reusable("Test (build only)", "tools/b", true),
        reusable("Test", "tools/b", false),
        reusable("Clippy", "tools/a", false),
    ];
    qualify_shared(&mut steps);
    assert_eq!(
        names(&steps),
        [
            "Format (tools/a)",
            "Test (tools/a) (build only)",
            "Test (tools/a)",
            "Format (tools/b)",
            "Test (tools/b) (build only)",
            "Test (tools/b)",
            "Clippy",
        ]
    );
}

#[test]
fn a_reusable_step_beside_a_user_step_of_its_name_is_refused() {
    let mut steps = vec![reusable("Format", ".", false), user("Format")];
    qualify_shared(&mut steps);
    assert_eq!(
        violations(steps),
        [Rule::ReusableStepNameShared("Format".to_owned())]
    );
}

#[test]
fn a_build_only_twin_named_like_a_user_step_is_refused() {
    let steps = vec![
        reusable("Test (build only)", ".", true),
        user("Test (build only)"),
    ];
    assert_eq!(
        violations(steps),
        [Rule::ReusableStepNameShared("Test (build only)".to_owned())]
    );
}

fn violations(steps: Vec<RenderedStep>) -> Vec<Rule> {
    let job = RenderedJob {
        id: "rust-root".to_owned(),
        steps,
        ..RenderedJob::default()
    };
    check_rules(&RenderedWorkflow { jobs: vec![job] })
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

#[test]
fn a_qualified_name_that_a_user_step_already_holds_is_refused() {
    let mut steps = vec![
        reusable("Format", "tools/a", false),
        reusable("Format", "tools/b", false),
        user("Format (tools/a)"),
    ];
    qualify_shared(&mut steps);
    assert_eq!(
        violations(steps),
        [Rule::ReusableStepNameShared("Format (tools/a)".to_owned())]
    );
}

#[test]
fn qualified_names_that_no_user_step_holds_pass() {
    let mut steps = vec![
        reusable("Format", "tools/a", false),
        reusable("Format", "tools/b", false),
        user("Format (tools/c)"),
    ];
    qualify_shared(&mut steps);
    assert_eq!(violations(steps), []);
}

#[test]
fn a_filter_the_word_splitter_cannot_parse_leaves_no_twin() {
    for run in [
        "cargo nextest run -E \"test(/a/) & $(echo 'x')\"",
        "cargo nextest run -E $'test(/a/)'",
        "cargo nextest run -E 'unbalanced",
        "cargo nextest run -E `echo a`",
        "cargo nextest run -E \"test($(date +\\\"%s\\\"))\"",
        "cargo nextest run -E a\\",
    ] {
        assert_eq!(build_only(run), None, "{run}");
    }
}

#[test]
fn a_quoted_filter_with_a_flag_inside_stays_whole_in_the_twin() {
    let run = "cargo nextest run --no-tests=fail -E 'test(/ --no-tests=x/)' '$(literal)'";
    assert_eq!(
        build_only(run).as_deref(),
        Some("cargo nextest run -E 'test(/ --no-tests=x/)' '$(literal)' --no-run")
    );
}

fn test_step() -> Result<Step, Box<dyn std::error::Error>> {
    Ok(Step {
        kind: StepKind::Cargo(CargoCommand::NextestRun {
            filter: FilterRef::All,
            ci_profile: false,
        }),
        directory: None,
        manifest: None,
        features: LaneFeatures::default(),
        target_dir: None,
        when: When::Success,
        pr: PrRun::Always,
        gate: None,
        reads: None,
        setup_order: SetupOrder::Unordered,
        skip_after_merge_group: None,
        reuse: Some(Reuse {
            id: "root.test-1".to_owned(),
            plan: JobId::new("rust-plan")?,
        }),
        skip_when_cold: false,
        private_repo: false,
        hosted_only: false,
        any_gate: Vec::new(),
    })
}

#[test]
fn with_twin_leaves_the_twin_without_a_command_when_the_run_cannot_be_split()
-> Result<(), Box<dyn std::error::Error>> {
    let body = |run: &str| RenderedStep {
        name: "Test".to_owned(),
        run: Some(run.to_owned()),
        ..RenderedStep::default()
    };
    let steps = with_twin(&test_step()?, body("cargo nextest run -E 'unbalanced"));
    assert_eq!(names(&steps), ["Test (build only)", "Test"]);
    assert_eq!(steps[0].run, None);
    let steps = with_twin(&test_step()?, body("cargo nextest run"));
    assert_eq!(steps[0].run.as_deref(), Some("cargo nextest run --no-run"));
    Ok(())
}
