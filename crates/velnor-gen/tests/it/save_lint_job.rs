// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The `lint-test` shape: its lint job holds no build or test step, and its saves wait for
//! every compile step of that job, the `Check <pkg> alone` steps included.

use velnor_model::{Target, TargetKind, TargetName};

use crate::archive::jobs_of;
use crate::base_save::{names, steps};
use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};
use crate::save_after_compile::{saves, with_outcomes};

const HELPER: &str = "0.1.109";

fn lint_jobs() -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut repo = view(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(b"shape = \"lint-test\"\n"),
    )?;
    for metadata in repo.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            member.targets.push(Target {
                name: TargetName::new("lib")?,
                kinds: vec![TargetKind::Lib],
                tests: false,
                doctest: false,
                source: None,
                requires_features: false,
            });
        }
    }
    pin_helper(&mut repo, HELPER)?;
    let lanes: Vec<_> = jobs_of(&workflow(&repo)?)
        .into_iter()
        .filter(|(_, block)| block.contains("name: Save the target cache"))
        .filter(|(_, block)| !block.contains("name: Test"))
        .collect();
    assert!(!lanes.is_empty(), "no lint job saves");
    Ok(lanes)
}

fn compile_ids(lane: &str) -> Vec<String> {
    steps(lane)
        .into_iter()
        .filter_map(|step| {
            step.lines()
                .find_map(|line| line.trim().strip_prefix("id: compile_").map(str::to_owned))
        })
        .map(|id| format!("compile_{id}"))
        .collect()
}

#[test]
fn the_alone_checks_come_before_the_saves() -> TestResult {
    for (id, lane) in lint_jobs()? {
        let order = names(&lane);
        let first_save = order
            .iter()
            .position(|name| name == "velnor-ci inputs save")
            .ok_or("no inputs save")?;
        let alone: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, name)| name.ends_with(" alone"))
            .map(|(at, _)| at)
            .collect();
        assert!(!alone.is_empty(), "{id}: no alone check in {order:?}");
        assert!(alone.iter().all(|at| *at < first_save), "{id}: {order:?}");
    }
    Ok(())
}

#[test]
fn every_compile_step_of_the_lint_job_has_an_id_the_saves_wait_for() -> TestResult {
    for (id, lane) in lint_jobs()? {
        let ids = compile_ids(&lane);
        let compiles = steps(&lane)
            .into_iter()
            .filter(|step| {
                step.lines().any(|line| {
                    let name = line.trim_start_matches("- ").trim();
                    name == "name: Lint"
                        || name == "name: Documentation"
                        || (name.starts_with("name: Check ") && name.ends_with(" alone"))
                })
            })
            .count();
        assert!(compiles >= 3, "{id}: {compiles}");
        assert_eq!(ids.len(), compiles, "{id}: {ids:?}");
        let record = crate::base_save::step(&lane, crate::save_after_compile::RECORD)
            .ok_or("no record step")?;
        for compile in &ids {
            let term = format!("steps.{compile}.outcome != ''failure''");
            assert!(record.contains(&term), "{id} {term}: {record}");
        }
    }
    Ok(())
}

#[test]
fn a_failed_lint_doc_or_alone_check_saves_nothing() -> TestResult {
    for (id, lane) in lint_jobs()? {
        let ids = compile_ids(&lane);
        assert_eq!(ids.len(), 3, "{id}: {ids:?}");
        for failed in &ids {
            for job_ok in [true, false] {
                let mut context = with_outcomes(&ids, Some(failed), "success");
                context.job_ok = job_ok;
                let saved = saves(&lane, context)?;
                assert!(
                    saved.iter().all(|ran| !ran),
                    "{id} {failed} job_ok={job_ok}: {saved:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn green_compile_steps_still_save_on_the_default_branch() -> TestResult {
    for (id, lane) in lint_jobs()? {
        let context = with_outcomes(&compile_ids(&lane), None, "success");
        let saved = saves(&lane, context)?;
        assert!(saved.iter().all(|ran| *ran), "{id}: {saved:?}");
    }
    Ok(())
}
