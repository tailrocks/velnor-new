// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
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
fn budget_runs_after_saves_with_configured_bytes_and_read_permission() -> TestResult {
    for (config, bytes) in [
        ("full_run = \"off\"\n", 8_000_000_000_u64),
        ("full_run = \"off\"\ncache_budget_bytes = 123\n", 123),
    ] {
        let text = render("0.1.81", config)?;
        let all = jobs_of(&text);
        let lane = &all.iter().find(|(id, _)| id == "rust").ok_or("rust")?.1;
        let steps: Vec<_> = lane.split("\n      - ").skip(1).collect();
        let saves: Vec<_> = steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.contains("name: Save the"))
            .collect();
        assert_eq!(saves.len(), 3);
        for (index, saved) in saves {
            let next = steps[index + 1];
            assert!(
                next.contains(&format!("velnor-ci cache budget --budget-bytes {bytes}")),
                "{saved}\n{next}"
            );
            assert!(
                next.contains("github.ref == format(''refs/heads/{0}''"),
                "{next}"
            );
            assert!(!next.contains("pull_request"), "{next}");
            assert!(next.contains("GITHUB_TOKEN:"), "{next}");
        }
        assert!(lane.contains("actions: write"));
        assert!(lane.contains("pull-requests: read"));
        assert!(!text.contains("rust-cache-budget:"));
    }
    for invalid in ["-1", "\"800\"", "1.5", "18446744073709551616"] {
        assert!(render("0.1.81", &format!("cache_budget_bytes = {invalid}\n")).is_err());
    }
    assert!(render("0.1.81", "cache_budget_bytes = 0\n").is_ok());
    Ok(())
}

#[test]
fn scheduled_budget_waits_for_all_jobs_without_selection_and_old_helpers_omit_it() -> TestResult {
    let text = render("0.1.81", "shape = \"groups\"\nfull_run = \"hourly\"\n")?;
    let all = jobs_of(&text);
    let (_, scheduled) = all
        .iter()
        .find(|(id, _)| id == "rust-cache-budget")
        .ok_or("maintenance")?;
    assert!(
        scheduled.contains("github.event_name == ''schedule''"),
        "{scheduled}"
    );
    assert!(scheduled.contains("!cancelled()"), "{scheduled}");
    assert!(
        scheduled.contains("github.ref == format(''refs/heads/{0}''"),
        "{scheduled}"
    );
    assert!(!scheduled.contains("outputs.rust"), "{scheduled}");
    for (id, _) in &all {
        if id != "rust-cache-budget" && id != "required" {
            assert!(scheduled.contains(id), "{id}: {scheduled}");
        }
    }
    assert!(scheduled.contains("actions: write"));
    assert!(scheduled.contains("pull-requests: read"));
    let old = render("0.1.80", "")?;
    assert!(!old.contains("cache budget"));
    assert!(!old.contains("rust-cache-budget:"));
    Ok(())
}
