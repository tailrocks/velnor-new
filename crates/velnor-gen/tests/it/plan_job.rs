// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn steps_of<'t>(text: &'t str, job: &str) -> Vec<&'t str> {
    let header = format!("  {job}:\n");
    let start = text
        .find(&header)
        .map_or(text.len(), |at| at + header.len());
    let body = &text[start..];
    body.lines()
        .take_while(|line| {
            !(line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        })
        .filter_map(|line| {
            line.strip_prefix("      - name: ")
                .or_else(|| line.strip_prefix("        name: "))
        })
        .collect()
}

const LANE_SETUP: [&str; 17] = [
    "Check out",
    "Resolve released helper tools",
    "Install velnor-ci 0.1.81",
    "Install the released helper launcher",
    "Private mbx store",
    "Restore the tools cache",
    "Install tools",
    "Add Rust components",
    "Save the tools cache",
    "velnor-ci prepare",
    "velnor-ci check-metadata",
    "Hash the sources lockfiles",
    "Restore the sources cache",
    "Restore the target cache",
    "velnor-ci inputs restore",
    "velnor-ci restored",
    "velnor-ci plan --full",
];

#[test]
fn a_single_job_plans_first_and_every_later_step_waits_for_a_successful_plan_that_selects_something()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!text.contains("rust-plan"), "{text}");
    let steps = steps_of(&text, "rust");
    assert_eq!(
        steps[..5],
        [
            "Check out",
            "Resolve released helper tools",
            "Install velnor-ci 0.1.81",
            "Install the released helper launcher",
            "velnor-ci plan",
        ]
    );
    let plan_index = steps
        .iter()
        .position(|name| *name == "velnor-ci plan")
        .ok_or("no plan")?;
    let condition = "steps.plan.outcome == ''success'' && (steps.replan.outputs.rust_root_mode || steps.plan.outputs.rust_root_mode) != ''none''";
    let plain =
        "steps.plan.outcome == ''success'' && steps.plan.outputs.rust_root_mode != ''none''";
    let gated = text.matches(plain).count() + text.matches(condition).count();
    let reporting = steps
        .iter()
        .filter(|name| matches!(**name, "Report cache telemetry" | "Upload cache metrics"))
        .count();
    let later = steps.len() - plan_index - 1 - reporting;
    assert_eq!(
        gated, later,
        "every step after the plan carries the gate: {text}"
    );
    assert!(
        text.contains(&format!(
            "        if: '${{{{ {plain} }}}}'\n        uses: jdx/mise-action@"
        )),
        "{text}"
    );
    assert!(text.contains(r#""$VELNOR_EXEC" plan --explain'"#));
    assert!(!text.contains("steps.velnor-helper-check-"));
    Ok(())
}

#[test]
fn more_than_one_job_start_a_plan_job_that_installs_nothing_and_every_lane_waits_for_it()
-> TestResult {
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let plan_steps = steps_of(&text, "rust-plan");
    assert_eq!(
        plan_steps[..3],
        [
            "Check out",
            "Resolve released helper tools",
            "Install velnor-ci 0.1.81"
        ]
    );
    assert_eq!(plan_steps.last(), Some(&"velnor-ci plan"));
    let plan_job = &text[text.find("  rust-plan:").unwrap_or_default()
        ..text.find("  rust-root:").unwrap_or_default()];
    for forbidden in [
        "mise-action",
        "rustup",
        "cargo",
        "prepare",
        "check-metadata",
    ] {
        assert!(!plan_job.contains(forbidden), "{forbidden} in {plan_job}");
    }
    for slug in ["root", "lab"] {
        for output in ["mode", "docs"] {
            assert!(
                text.contains(&format!(
                    "      rust_{slug}_{output}: '${{{{ steps.plan.outputs.rust_{slug}_{output} }}}}'\n"
                )),
                "{slug} {output}"
            );
        }
        assert!(text.contains(&format!(
            "    needs:\n      - rust-plan\n      - rust-inputs-audit\n    if: '${{{{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (!cancelled() && needs.rust-plan.result == ''success'' && needs.rust-plan.outputs.rust_{slug}_mode != ''none'') && (needs.rust-inputs-audit.result == ''success'' || needs.rust-inputs-audit.result == ''skipped'') }}}}'\n"
        )));
    }
    assert!(!text.contains("_doctests"), "{text}");
    let lane = steps_of(&text, "rust-lab");
    assert_eq!(lane[..LANE_SETUP.len()], LANE_SETUP);
    assert!(lane[LANE_SETUP.len()].starts_with("Format"), "{lane:?}");
    assert!(text.contains(
        "      - rust-plan\n      - rust-root\n      - rust-lab\n      - rust-inputs-audit\n    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (always()) }}'\n"
    ));
    Ok(())
}

#[test]
fn the_doctest_and_documentation_steps_follow_the_flags_the_plan_writes() -> TestResult {
    let mut files = view(&["Cargo.toml"], None)?;
    let lib = RepoPath::new("src/lib.rs")?;
    for package in files
        .rust_metadata
        .values_mut()
        .flat_map(|m| &mut m.members)
    {
        for target in &mut package.targets {
            target.kinds = vec![velnor_model::TargetKind::Lib];
            target.source = Some(lib.clone());
            target.doctest = true;
        }
    }
    let text = workflow(&files)?;
    let doctests = "(steps.replan.outputs.rust_root_doctests || steps.plan.outputs.rust_root_doctests) == ''run''";
    let docs =
        "(steps.replan.outputs.rust_root_docs || steps.plan.outputs.rust_root_docs) == ''run''";
    assert!(text.contains(doctests), "{text}");
    assert!(text.contains(docs), "{text}");
    let known = "DOCTESTS_KNOWN: '${{ steps.replan.outputs.rust_root_doctests_known || steps.plan.outputs.rust_root_doctests_known }}'";
    assert!(text.contains(known), "{text}");
    let steps: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("cargo test --doc"))
        .collect();
    assert_ne!(steps.len(), 0);
    for line in steps {
        assert!(
            line.contains(
                "\"$VELNOR_EXEC\" run --require-doctests=$DOCTESTS_KNOWN -- velnor-ci inputs build -- cargo test --doc"
            ),
            "{line}"
        );
    }
    Ok(())
}

mod checks;
