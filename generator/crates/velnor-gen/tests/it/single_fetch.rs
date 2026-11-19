// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{RepoPath, RepoView};

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const FETCH_STEP: &str = "Fetch the sources of every root";
const SAVE_STEP: &str = "Save the sources cache";

fn repo(manifests: &[&str], helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(manifests, None)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    repo.files
        .insert(lock, text.replace("0.1.82", helper).into_bytes());
    Ok(repo)
}

fn many(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("services/payments-gateway-region-{index:02}/Cargo.toml")
            }
        })
        .collect()
}

fn savers(text: &str) -> Vec<(String, String)> {
    jobs_of(text)
        .into_iter()
        .filter(|(_, block)| step(block, SAVE_STEP).is_some())
        .collect()
}

#[test]
fn a_helper_with_sources_fetch_gets_one_step_listing_every_root_in_order() -> TestResult {
    let text = workflow(&repo(
        &["Cargo.toml", "xtask/Cargo.toml", "tools/Cargo.toml"],
        "0.1.86",
    )?)?;
    let savers = savers(&text);
    assert!(savers.len() >= 3, "{}", savers.len());
    for (id, block) in savers {
        let all = names(&block);
        let fetches: Vec<&String> = all
            .iter()
            .filter(|name| name.starts_with("Fetch the sources of "))
            .collect();
        assert_eq!(fetches, [FETCH_STEP], "{id}");
        let fetch = step(&block, FETCH_STEP).ok_or("no fetch")?;
        assert!(
            fetch
                .lines()
                .any(|line| line.trim() == "run: velnor-ci sources fetch --roots . ./tools ./xtask"),
            "{id}: {fetch}"
        );
        assert!(!fetch.contains("working-directory"), "{fetch}");
        assert!(fetch.contains("continue-on-error: true"), "{fetch}");
        assert!(
            fetch.contains("steps.base.outputs.sources == ''true''"),
            "{fetch}"
        );
        assert!(fetch.contains("cache-hit != ''true''"), "{fetch}");
        let position = |wanted: &str| all.iter().position(|name| name == wanted);
        assert!(position(FETCH_STEP) < position(SAVE_STEP), "{id}");
    }
    Ok(())
}

#[test]
fn the_sources_save_still_requires_the_one_fetch_to_succeed() -> TestResult {
    let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], "0.1.86")?)?;
    let savers = savers(&text);
    assert!(savers.len() >= 2, "{}", savers.len());
    for (id, block) in savers {
        let save = step(&block, SAVE_STEP).ok_or("no save")?;
        assert!(
            save.contains("steps.sources_fetch_all.outcome == ''success''"),
            "{id}: {save}"
        );
        let fetch = step(&block, FETCH_STEP).ok_or("no fetch")?;
        assert!(fetch.contains("id: sources_fetch_all"), "{id}: {fetch}");
    }
    Ok(())
}

#[test]
fn a_helper_without_sources_fetch_keeps_one_step_per_root_and_the_sixteen_root_refusal()
-> TestResult {
    let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], "0.1.81")?)?;
    for (id, block) in savers(&text) {
        assert!(step(&block, FETCH_STEP).is_none(), "{id}");
        for position in 0..2 {
            let fetch = step(&block, &format!("Fetch the sources of root {position}"))
                .ok_or("no per-root fetch")?;
            assert!(
                fetch.contains("velnor-ci run -- cargo fetch --locked"),
                "{id}: {fetch}"
            );
        }
    }
    let manifests = many(17);
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    match expected_with_notes(&repo(&names, "0.1.81")?) {
        Err(GenerateError::TooManySourceRoots(17)) => Ok(()),
        other => Err(format!("17 roots with 0.1.81: {:?}", other.map(|_| ())).into()),
    }
}

#[test]
fn the_released_helpers_without_sources_fetch_keep_the_per_root_steps_and_the_limit() -> TestResult
{
    for helper in ["0.1.82", "0.1.83", "0.1.84", "0.1.85"] {
        let text = workflow(&repo(&["Cargo.toml", "xtask/Cargo.toml"], helper)?)?;
        for (id, block) in savers(&text) {
            assert!(step(&block, FETCH_STEP).is_none(), "{helper} {id}");
            assert!(
                step(&block, "Fetch the sources of root 1").is_some(),
                "{helper} {id}"
            );
        }
        let manifests = many(17);
        let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
        match expected_with_notes(&repo(&names, helper)?) {
            Err(GenerateError::TooManySourceRoots(17)) => {}
            other => return Err(format!("{helper}: {:?}", other.map(|_| ())).into()),
        }
    }
    Ok(())
}

#[test]
fn twenty_roots_fit_the_workflow_limit_with_the_single_fetch_step() -> TestResult {
    let manifests = many(20);
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let text = workflow(&repo(&names, "0.1.86")?)?;
    assert!(text.len() <= 500_000, "{} bytes", text.len());
    let savers = savers(&text);
    assert!(savers.len() >= 20, "{}", savers.len());
    let (_, block) = &savers[0];
    let fetch = step(block, FETCH_STEP).ok_or("no fetch")?;
    let run = fetch
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or("no run")?;
    assert_eq!(
        run.split_whitespace().count(),
        4 + 20,
        "one word per root: {run}"
    );
    Ok(())
}
