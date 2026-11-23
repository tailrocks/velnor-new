// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::base_save::step;
use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};

#[test]
fn every_lane_with_the_privacy_step_tells_the_report_its_own_plan_mode() -> TestResult {
    let mut repo = private_repo(
        &["Cargo.toml", "xtask/Cargo.toml"],
        &format!("shape = \"lint-test\"\n{SECTION}"),
        "0.1.112",
    )?;
    for lock in ["Cargo.lock", "xtask/Cargo.lock"] {
        repo.files.insert(path(lock)?, GIT_LOCK.as_bytes().to_vec());
    }
    let text = workflow(&repo)?;
    let mut seen = Vec::new();
    for (id, block) in jobs_of(&text) {
        if step(&block, "Check that the repository is private").is_none() {
            continue;
        }
        let report = step(&block, "Report cache telemetry").ok_or("no report step")?;
        let mode = report
            .lines()
            .find_map(|line| line.trim().strip_prefix("VELNOR_REPORT_PLAN_MODE: "))
            .ok_or_else(|| format!("{id}: the report step has no plan mode\n{report}"))?;
        assert!(mode.contains("_mode"), "{id}: {mode}");
        seen.push((id, mode.to_owned()));
    }
    assert!(seen.len() >= 2, "{seen:?}");
    let distinct: std::collections::BTreeSet<_> = seen.iter().map(|(_, mode)| mode).collect();
    assert!(
        distinct.len() >= 2,
        "each workspace reads its own mode: {seen:?}"
    );
    Ok(())
}
