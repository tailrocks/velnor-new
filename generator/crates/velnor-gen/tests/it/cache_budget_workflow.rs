// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::base_save::step;
use crate::facts::TestResult;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};
use velnor_model::{RepoPath, TargetKind};
use velnor_rust::{LOCK_PATH, pin_lock};

fn render(version: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    repo.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS, None)?.into_bytes(),
    );
    workflow(&repo)
}

#[test]
fn budget_runs_once_after_all_saves_with_configured_bytes_and_read_permission() -> TestResult {
    for (config, bytes) in [
        ("full_run = \"off\"\n", 8_000_000_000_u64),
        ("full_run = \"off\"\ncache_budget_bytes = 123\n", 123),
    ] {
        let text = render("0.1.81", config)?;
        let all = jobs_of(&text);
        let trim = &all
            .iter()
            .find(|(id, _)| id == "rust-trim")
            .ok_or("trim")?
            .1;
        assert_eq!(text.matches("\"$VELNOR_EXEC\" cache budget").count(), 1);
        let budget = step(trim, "Enforce the repository cache budget").ok_or("budget")?;
        assert!(
            budget.contains(&format!("velnor-ci cache budget --budget-bytes {bytes}")),
            "{budget}"
        );
        assert!(budget.contains("GITHUB_TOKEN:"));
        assert!(!budget.contains("    if:"), "{budget}");
        assert!(trim.ends_with(budget), "{trim}");
        assert!(trim.contains("actions: write"));
        assert!(trim.contains("pull-requests: read"));
        for (id, block) in &all {
            if block.contains("name: Save the") {
                assert!(trim.contains(&format!("      - {id}\n")), "{id}: {trim}");
                assert!(!block.contains("actions: write"), "{id}");
                assert!(!block.contains("cache budget"), "{id}");
            }
        }
        let required = &all
            .iter()
            .find(|(id, _)| id == "required")
            .ok_or("required")?
            .1;
        assert!(!required.contains("rust-trim"));
        assert!(!text.contains("rust-cache-budget:"));
    }
    for invalid in ["-1", "\"800\"", "1.5", "18446744073709551616", "8000000001"] {
        assert!(render("0.1.81", &format!("cache_budget_bytes = {invalid}\n")).is_err());
    }
    for value in [0_u64, 8_000_000_000] {
        assert!(render("0.1.81", &format!("cache_budget_bytes = {value}\n")).is_ok());
    }
    Ok(())
}

#[test]
fn scheduled_budget_runs_after_writers_even_when_a_sibling_or_all_saves_are_skipped() -> TestResult
{
    let text = render("0.1.81", "shape = \"groups\"\nfull_run = \"hourly\"\n")?;
    let all = jobs_of(&text);
    let (_, trim) = all.iter().find(|(id, _)| id == "rust-trim").ok_or("trim")?;
    let condition = trim
        .lines()
        .find(|line| line.starts_with("    if:"))
        .ok_or("condition")?;
    assert!(
        condition.contains("github.event_name == ''schedule''"),
        "{condition}"
    );
    assert!(condition.contains("!cancelled()"), "{condition}");
    assert!(
        condition.contains("github.ref == format(''refs/heads/{0}''"),
        "{condition}"
    );
    assert!(!condition.contains("needs."), "{condition}");
    assert!(!condition.contains("success()"), "{condition}");
    assert!(!trim.contains("checkout"), "{trim}");
    assert!(!trim.contains("cargo "), "{trim}");
    let budget = step(trim, "Enforce the repository cache budget").ok_or("budget")?;
    assert!(!budget.contains("base_save"), "{budget}");
    assert!(!budget.contains("outputs.rust"), "{budget}");
    let mut writers = 0;
    for (id, block) in &all {
        if block.contains("name: Save the") {
            writers += 1;
            assert!(trim.contains(&format!("      - {id}\n")), "{id}: {trim}");
        }
    }
    assert!(writers > 1);
    let old = render("0.1.80", "full_run = \"off\"\n")?;
    assert!(!old.contains("cache budget"));
    assert!(!old.contains("rust-trim:"));
    Ok(())
}

#[test]
fn a_repository_without_cache_writers_still_budgets_existing_caches() -> TestResult {
    for bytes in [0_u64, 8_000_000_000] {
        for version in ["0.1.80", "0.1.81"] {
            let mut repo = crate::base_save::pinned(
                version,
                &format!("full_run = \"off\"\ncache_budget_bytes = {bytes}\n"),
            )?;
            repo.files
                .insert(RepoPath::new("mise.toml")?, b"[tools]\n".to_vec());
            repo.files.insert(
                RepoPath::new("Cargo.lock")?,
                b"version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"private\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/example/private#0123456789abcdef\"\n".to_vec(),
            );
            for target in repo
                .rust_metadata
                .values_mut()
                .flat_map(|metadata| &mut metadata.members)
                .flat_map(|member| &mut member.targets)
            {
                target.kinds = vec![TargetKind::Lib];
                target.tests = false;
                target.doctest = false;
            }
            let text = workflow(&repo)?;
            assert!(!text.contains("name: Save the"), "{text}");
            if version == "0.1.80" {
                assert!(!text.contains("rust-trim:"), "{text}");
                assert!(!text.contains("cache budget"), "{text}");
                continue;
            }
            let all = jobs_of(&text);
            let (_, trim) = all
                .iter()
                .find(|(id, _)| id == "rust-trim")
                .ok_or("no cache budget job when no cache writers exist")?;
            for absent in [
                "    needs:",
                "cache trim",
                "checkout",
                "cargo ",
                "download-artifact",
            ] {
                assert!(!trim.contains(absent), "{absent}: {trim}");
            }
            assert!(trim.contains("Install velnor-ci 0.1.81"), "{trim}");
            assert!(trim.contains("actions: write"), "{trim}");
            assert!(trim.contains("pull-requests: read"), "{trim}");
            let condition = trim
                .lines()
                .find(|line| line.starts_with("    if:"))
                .ok_or("no budget job condition")?;
            for required in [
                "!cancelled()",
                "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)",
                "github.event_name == ''push''",
                "github.event_name == ''schedule''",
                "github.event_name == ''workflow_dispatch''",
            ] {
                assert!(condition.contains(required), "{required}: {condition}");
            }
            assert!(!condition.contains("needs."), "{condition}");
            assert_eq!(text.matches("\"$VELNOR_EXEC\" cache budget").count(), 1);
            let budget = step(trim, "Enforce the repository cache budget").ok_or("budget")?;
            assert!(
                budget.contains(&format!("velnor-ci cache budget --budget-bytes {bytes}")),
                "{budget}"
            );
            assert!(!budget.contains("    if:"), "{budget}");
            let (_, required) = all
                .iter()
                .find(|(id, _)| id == "required")
                .ok_or("required")?;
            assert!(!required.contains("rust-trim"), "{required}");
        }
    }
    Ok(())
}

#[test]
fn decide_gets_the_configured_cache_budget() -> TestResult {
    for (config, bytes) in [
        ("full_run = \"off\"\n", "8000000000"),
        ("full_run = \"off\"\ncache_budget_bytes = 123\n", "123"),
    ] {
        let text = render("0.1.81", config)?;
        let mut lanes = 0;
        for (_, block) in jobs_of(&text) {
            let Some(decide) = step(&block, "Decide whether to save the build directory") else {
                continue;
            };
            lanes += 1;
            assert!(
                decide.contains(&format!("VELNOR_CACHE_BUDGET_BYTES: '{bytes}'"))
                    || decide.contains(&format!("VELNOR_CACHE_BUDGET_BYTES: {bytes}")),
                "{decide}"
            );
        }
        assert!(lanes > 0, "{text}");
    }
    Ok(())
}
