// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::{
    CACHE_VERSION_RUNNER_TEMP, CacheFamily, RUNNER_TEMP_EXPR,
};
use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::mbx_repo;

fn path_input(step: &Step) -> Result<&str, Box<dyn Error>> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Err("sources cache step is not an action".into());
    };
    with.get("path")
        .map(String::as_str)
        .ok_or_else(|| "sources cache step has no path".into())
}

#[test]
fn every_sources_restore_spells_the_layout_its_writer_saves() -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let writer_paths = CacheFamily::Sources.spec().path_input(None);
    let writer_version_paths = writer_paths.replace(RUNNER_TEMP_EXPR, CACHE_VERSION_RUNNER_TEMP);
    let mut restores = 0;
    let mut saves = 0;
    for job in jobs.values() {
        for step in &job.steps {
            match step.role {
                Some(StepRole::CargoSourcesRestore) => {
                    restores += 1;
                    assert_eq!(path_input(step)?, writer_version_paths);
                    assert!(!path_input(step)?.contains("runner.temp"));
                }
                Some(StepRole::CargoSourcesSave) => {
                    saves += 1;
                    assert_eq!(path_input(step)?, writer_paths);
                }
                _ => {}
            }
        }
    }
    assert!(restores >= 2, "restores={restores}");
    assert_eq!(saves, 1);
    Ok(())
}
