// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0


use velnor_model::RepoPath;

use crate::archive::jobs_of;
use crate::base_save::step;
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const BUDGET_BYTES: usize = 470_000;

fn twenty_roots(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let manifests: Vec<String> = (0..20)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("services/payments-gateway-region-{index:02}/Cargo.toml")
            }
        })
        .collect();
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let mut repo = view(&roots, None)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    repo.files
        .insert(lock, text.replace("0.1.81", helper).into_bytes());
    workflow(&repo)
}

fn root_job(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs_of(text)
        .into_iter()
        .find(|(id, _)| id.ends_with("region_18"))
        .map(|(_, block)| block)
        .ok_or_else(|| "no root job".into())
}

fn condition(block: &str, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let step = step(block, name).ok_or("no step")?;
    step.lines()
        .find_map(|line| line.trim().strip_prefix("if: "))
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} has no if").into())
}

#[test]
fn twenty_roots_at_helper_0_1_108_stay_under_the_size_budget() -> TestResult {
    let text = twenty_roots("0.1.109")?;
    assert!(
        text.len() <= BUDGET_BYTES,
        "{} bytes, budget {BUDGET_BYTES}",
        text.len()
    );
    Ok(())
}

#[test]
fn the_saves_after_the_decision_read_its_outputs_instead_of_repeating_its_conditions() -> TestResult
{
    let block = root_job(&twenty_roots("0.1.109")?)?;
    assert_eq!(
        condition(&block, "Decide whether to save the build directory")?,
        "'${{ !cancelled() && steps.inputs_save.outcome == ''success'' }}'"
    );
    let target = "'${{ !cancelled() && steps.base.outputs.save == ''true'' && github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch) && steps.prepare.outputs.cold != ''true'' }}'";
    assert_eq!(condition(&block, "Save the target cache")?, target);
    assert_eq!(condition(&block, "Record the target cache key")?, target);
    let sources = condition(&block, "Save the sources cache")?;
    assert!(
        sources.starts_with(
            "'${{ (!cancelled() && steps.base.outputs.sources == ''true'' && github.ref == format("
        ),
        "{sources}"
    );
    assert!(
        sources.contains("steps.sources_fetch_all.outcome == ''success''"),
        "{sources}"
    );
    Ok(())
}

#[test]
fn the_decision_itself_keeps_every_condition_of_a_save() -> TestResult {
    let block = root_job(&twenty_roots("0.1.109")?)?;
    let saved = condition(&block, "velnor-ci inputs save")?;
    for needed in [
        "!cancelled()",
        "steps.test.outputs.built == ''true''",
        "steps.restored.outcome == ''success''",
        "steps.replan.outcome != ''failure''",
        "github.event.repository.default_branch",
        "github.event_name == ''push''",
    ] {
        assert!(saved.contains(needed), "{needed}: {saved}");
    }
    Ok(())
}

#[test]
fn an_older_helper_keeps_the_full_condition_on_every_save() -> TestResult {
    let block = root_job(&twenty_roots("0.1.107")?)?;
    for name in [
        "Decide whether to save the build directory",
        "Save the target cache",
        "Record the target cache key",
        "Save the sources cache",
    ] {
        let found = condition(&block, name)?;
        assert!(
            found.contains("steps.restored.outcome == ''success''"),
            "{name}: {found}"
        );
        assert!(
            found.contains("steps.test.outputs.built == ''true''"),
            "{name}: {found}"
        );
    }
    Ok(())
}
