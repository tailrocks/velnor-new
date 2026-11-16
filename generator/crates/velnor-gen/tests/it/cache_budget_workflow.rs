// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::base_save::step;
use crate::facts::TestResult;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};
use velnor_rust::{LOCK_PATH, pin_lock};

fn render(version: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    repo.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS)?.into_bytes(),
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
        assert_eq!(text.matches("velnor-ci cache budget").count(), 1);
        let budget = step(trim, "Enforce the repository cache budget").ok_or("budget")?;
        assert!(budget.contains(&format!("velnor-ci cache budget --budget-bytes {bytes}")));
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
    let old = render("0.1.80", "")?;
    assert!(!old.contains("cache budget"));
    assert!(!old.contains("rust-trim:"));
    Ok(())
}
