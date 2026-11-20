// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Runs the rendered save conditions against the outcomes a job can end with: a save
//! never follows a build that did not finish or an audit that was required and did
//! not pass (#2027, #2028), and still follows a failed test (#1970).

use crate::base_save::{jobs, step};
use crate::facts::TestResult;
use crate::gate_eval::{Context, condition_of, runs};

const SAVES: [&str; 4] = [
    "velnor-ci inputs save",
    "Decide whether to save the build directory",
    "Save the target cache",
    "Record the target cache key",
];

/// How the saves learn that the tests were built.
#[derive(Clone, Copy, Debug)]
enum Variant {
    /// A `Build the tests` step with the id `build_1`.
    BuildStep,
    /// The test step records `built` in its outputs.
    Recorded,
}

impl Variant {
    fn helper(self) -> &'static str {
        match self {
            Self::BuildStep => "0.1.81",
            Self::Recorded => "0.1.107",
        }
    }
}

const BOTH: [Variant; 2] = [Variant::BuildStep, Variant::Recorded];

fn audit_step(lane: &str) -> Result<&str, Box<dyn std::error::Error>> {
    crate::base_save::steps(lane)
        .into_iter()
        .find(|step| step.contains("name: velnor-ci inputs audit"))
        .ok_or_else(|| "no audit".into())
}

fn lane(variant: Variant) -> Result<String, Box<dyn std::error::Error>> {
    jobs(variant.helper(), "")?
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, lane)| lane)
        .ok_or_else(|| "no rust job".into())
}

struct Ending {
    event: &'static str,
    expired: bool,
    built: bool,
    test_failed: bool,
    /// What the audit does when it runs; `skipped` keeps it from running at all.
    audit: &'static str,
}

const SCHEDULED: Ending = Ending {
    event: "schedule",
    expired: false,
    built: true,
    test_failed: true,
    audit: "success",
};

/// The outcome of the audit step and whether each save runs.
fn play(
    variant: Variant,
    ending: &Ending,
) -> Result<(String, Vec<bool>), Box<dyn std::error::Error>> {
    let lane = lane(variant)?;
    let mut context = Context::new(&[
        ("github.ref", "refs/heads/main"),
        ("github.event.repository.default_branch", "main"),
        ("steps.restored.outcome", "success"),
        ("steps.plan.outcome", "success"),
        ("steps.plan.outputs.rust_root_mode", "full"),
        ("steps.replan.outcome", "skipped"),
        ("steps.base.outputs.save", "true"),
    ]);
    context.set("github.event_name", ending.event);
    context.set(
        "steps.restored.outputs.expired",
        &ending.expired.to_string(),
    );
    let (built, test) = match (ending.built, ending.test_failed) {
        (true, true) => ("success", "failure"),
        (true, false) => ("success", "success"),
        (false, _) => ("failure", "skipped"),
    };
    context.set("steps.build_1.outcome", built);
    context.set("steps.test.outcome", test);
    context.set(
        "steps.test.outputs.built",
        if ending.built { "true" } else { "" },
    );
    context.job_ok = ending.built && !ending.test_failed;
    let audit = audit_step(&lane)?;
    let outcome = if ending.audit != "skipped" && runs(&condition_of(audit)?, &context)? {
        ending.audit
    } else {
        "skipped"
    };
    context.set("steps.audit_1.outcome", outcome);
    context.job_ok &= outcome != "failure";
    if let Some(record) = step(&lane, "Record that the build can be saved") {
        let recorded = runs(&condition_of(record)?, &context)?;
        context.set(
            "steps.saveable.outcome",
            if recorded { "success" } else { "skipped" },
        );
    }
    let mut saves = Vec::new();
    for name in SAVES {
        let block = step(&lane, name).ok_or_else(|| format!("no {name} ({variant:?})"))?;
        let ran = runs(&condition_of(block)?, &context)?;
        if name == SAVES[0] {
            context.set(
                "steps.inputs_save.outcome",
                if ran { "success" } else { "skipped" },
            );
        }
        saves.push(ran);
    }
    Ok((outcome.to_owned(), saves))
}

fn all(saves: &[bool]) -> bool {
    saves.iter().all(|saved| *saved)
}

fn none(saves: &[bool]) -> bool {
    saves.iter().all(|saved| !*saved)
}

#[test]
fn a_successful_push_saves_without_an_audit() -> TestResult {
    for variant in BOTH {
        let ending = Ending {
            event: "push",
            test_failed: false,
            ..SCHEDULED
        };
        let (audit, saves) = play(variant, &ending)?;
        assert_eq!(audit, "skipped", "{variant:?}");
        assert!(all(&saves), "{variant:?}: {saves:?}");
    }
    Ok(())
}

#[test]
fn a_failed_test_after_a_green_build_and_audit_still_saves() -> TestResult {
    for variant in BOTH {
        for (event, expired) in [("schedule", false), ("push", true)] {
            let ending = Ending {
                event,
                expired,
                ..SCHEDULED
            };
            let (audit, saves) = play(variant, &ending)?;
            assert_eq!(audit, "success", "{variant:?} {event} {expired}");
            assert!(all(&saves), "{variant:?} {event} {expired}: {saves:?}");
        }
    }
    Ok(())
}

#[test]
fn the_audit_runs_after_a_failed_test_when_the_build_was_green() -> TestResult {
    for variant in BOTH {
        let lane = lane(variant)?;
        let audit = condition_of(audit_step(&lane)?)?;
        let mut context = Context::new(&[
            ("github.event_name", "schedule"),
            ("steps.plan.outcome", "success"),
            ("steps.plan.outputs.rust_root_mode", "full"),
            ("steps.build_1.outcome", "success"),
            ("steps.test.outputs.built", "true"),
        ]);
        context.job_ok = false;
        assert!(runs(&audit, &context)?, "{variant:?}: {audit}");
        context.set("steps.build_1.outcome", "skipped");
        context.set("steps.test.outputs.built", "");
        assert!(!runs(&audit, &context)?, "{variant:?}: {audit}");
    }
    Ok(())
}

#[test]
fn a_skipped_audit_that_was_required_saves_nothing() -> TestResult {
    for variant in BOTH {
        for (event, expired) in [("schedule", false), ("push", true)] {
            let ending = Ending {
                event,
                expired,
                audit: "skipped",
                ..SCHEDULED
            };
            let (audit, saves) = play(variant, &ending)?;
            assert_eq!(audit, "skipped");
            assert!(none(&saves), "{variant:?} {event} {expired}: {saves:?}");
        }
    }
    Ok(())
}

#[test]
fn a_failed_audit_saves_nothing() -> TestResult {
    for variant in BOTH {
        let ending = Ending {
            audit: "failure",
            ..SCHEDULED
        };
        let (_, saves) = play(variant, &ending)?;
        assert!(none(&saves), "{variant:?}: {saves:?}");
    }
    Ok(())
}

#[test]
fn a_build_that_did_not_finish_saves_nothing_and_is_not_audited() -> TestResult {
    for variant in BOTH {
        let ending = Ending {
            built: false,
            ..SCHEDULED
        };
        let (audit, saves) = play(variant, &ending)?;
        assert_eq!(audit, "skipped", "{variant:?}");
        assert!(none(&saves), "{variant:?}: {saves:?}");
    }
    Ok(())
}

const GROUPS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../generator/samples/workspace-groups/expected/.github/workflows/rust-ci.yml"
);

/// The saves of the `rust-tools` job when its three group builds ended as given and
/// each group is selected (`true`) or left out by the plan.
fn groups(
    builds: [&str; 3],
    selected: [bool; 3],
    job_ok: bool,
) -> Result<Vec<bool>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(GROUPS)?;
    let job = text.split("\n  rust-tools:\n").nth(1).ok_or("no job")?;
    let job = job.split("\n  rust-inputs-audit:\n").next().ok_or("end")?;
    let mut context = Context::new(&[
        ("github.ref", "refs/heads/main"),
        ("github.event.repository.default_branch", "main"),
        ("github.event_name", "push"),
        ("steps.restored.outcome", "success"),
        ("steps.replan.outcome", "skipped"),
        ("steps.base.outputs.save", "true"),
    ]);
    for (index, (outcome, selected)) in builds.iter().zip(selected).enumerate() {
        let group = ["a", "b", "c"][index];
        context.set(&format!("steps.build_{}.outcome", index + 1), outcome);
        context.set(
            &format!("needs.rust-plan.outputs.rust_tools_{group}_mode"),
            if selected { "full" } else { "none" },
        );
    }
    context.job_ok = job_ok;
    ["velnor-ci inputs save", "Save the target cache"]
        .iter()
        .map(|name| {
            let block = step(job, name).ok_or_else(|| format!("no {name}"))?;
            runs(&condition_of(block)?, &context)
        })
        .collect()
}

#[test]
fn builds_that_all_succeeded_save() -> TestResult {
    let saves = groups(["success"; 3], [true; 3], true)?;
    assert!(all(&saves), "{saves:?}");
    Ok(())
}

#[test]
fn builds_that_were_all_skipped_after_an_earlier_failure_save_nothing() -> TestResult {
    let saves = groups(["skipped"; 3], [true; 3], false)?;
    assert!(none(&saves), "{saves:?}");
    Ok(())
}

#[test]
fn a_group_the_plan_left_out_does_not_stop_the_save() -> TestResult {
    let saves = groups(["success", "skipped", "success"], [true, false, true], true)?;
    assert!(all(&saves), "{saves:?}");
    Ok(())
}

#[test]
fn a_plan_that_selects_no_group_saves_nothing() -> TestResult {
    let saves = groups(["skipped"; 3], [false; 3], true)?;
    assert!(none(&saves), "{saves:?}");
    Ok(())
}

#[test]
fn one_failed_build_saves_nothing() -> TestResult {
    let saves = groups(["success", "failure", "success"], [true; 3], false)?;
    assert!(none(&saves), "{saves:?}");
    Ok(())
}

#[test]
fn a_selected_group_whose_build_was_skipped_saves_nothing() -> TestResult {
    let saves = groups(["success", "skipped", "success"], [true; 3], false)?;
    assert!(none(&saves), "{saves:?}");
    Ok(())
}
