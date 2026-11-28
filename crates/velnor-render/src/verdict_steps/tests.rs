// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{build_only, qualify_shared};
use crate::rules::{RenderedJob, RenderedReuse, RenderedStep, RenderedWorkflow, Rule, check_rules};

fn reusable(name: &str, root: &str, build_only: bool) -> RenderedStep {
    RenderedStep {
        name: name.to_owned(),
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
fn qualify_shared_leaves_one_reusable_step_beside_a_user_step_of_its_name() {
    let mut steps = vec![reusable("Format", ".", false), user("Format")];
    qualify_shared(&mut steps);
    assert_eq!(names(&steps), ["Format", "Format"]);
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
        .filter(|rule| !matches!(rule, Rule::NotExactlyOneOfRunAndUses))
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
fn a_filter_the_word_splitter_cannot_parse_leaves_no_twin_and_is_refused() {
    for run in [
        "cargo nextest run -E \"test(/a/) & $(echo 'x')\"",
        "cargo nextest run -E $'test(/a/)'",
        "cargo nextest run -E 'unbalanced",
        "cargo nextest run -E `echo a`",
        "cargo nextest run -E \"test($(date +\\\"%s\\\"))\"",
    ] {
        assert_eq!(build_only(run), None, "{run}");
        let mut twin = reusable("Test (build only)", ".", true);
        twin.run = build_only(run);
        assert_eq!(
            violations(vec![twin]),
            [Rule::BuildOnlyTwinUnparsable(
                "Test (build only)".to_owned()
            )],
            "{run}"
        );
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
