// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, migrate_config, prepare, render_staged_tree,
};

use super::bun_routing::make_bun_repo;
use super::waste_oracle::files_of;

const DISABLED: &str = "false";
const PUSH: &str = "github.event_name == 'push'";
const PROTECTED: &str = "github.ref_protected == true";

#[test]
fn family_cache_saves_are_disabled_outside_the_named_writers() -> Result<(), Box<dyn Error>> {
    for repo in [
        make_bun_repo()?,
        super::nonrust_family_fixture::repository()?,
    ] {
        verify_suppression(&repo)?;
    }
    Ok(())
}

fn verify_suppression(repo: &tempfile::TempDir) -> Result<(), Box<dyn Error>> {
    migrate_config(repo.path(), 2, true)?;
    let prepared = prepare(repo.path())?;
    let jobs = finalized_jobs(&prepared)?;
    let mut family_saves = 0;
    let mut writer_saves = 0;
    for (id, job) in &jobs {
        for step in &job.steps {
            let StepKind::Action { uses, .. } = &step.kind else {
                continue;
            };
            if !uses.starts_with("actions/cache/save@") {
                continue;
            }
            let condition = step.condition.as_deref().unwrap_or_default();
            if job.cache_access.is_read() {
                assert_eq!(condition, DISABLED, "{id}: {}", step.name);
                family_saves += usize::from(step.role == Some(StepRole::FamilySave));
            } else {
                assert!(
                    condition.contains(PUSH) && condition.contains(PROTECTED),
                    "{id}: writer save lost its gate: {condition}"
                );
                writer_saves += 1;
            }
        }
    }
    assert!(family_saves > 0, "no family save in a read job");
    assert!(writer_saves > 0, "no writer save kept its gate");
    let rendered = files_of(&render_staged_tree(&prepared)?)?;
    let workflow = rendered
        .get(".github/workflows/ci.yml")
        .ok_or("ci.yml not rendered")?;
    let output = tempfile::tempdir()?;
    generate(
        &prepared,
        &GenerateOptions {
            output_dir: Some(output.path().to_owned()),
        },
    )?;
    assert!(!workflow.contains("actions/cache/save@"));
    assert!(!workflow.contains("Admit optional cache save"));
    assert!(!workflow.contains("Record optional cache budget"));
    let producer = rendered
        .get(".github/workflows/state-seal.yml")
        .ok_or("missing producer workflow")?;
    for required in [
        "Admit optional cache save",
        "Record optional cache budget",
        "Report optional cache save failure",
        "timeout-minutes: 1",
        "continue-on-error: true",
    ] {
        assert!(
            producer.contains(required),
            "missing active producer budget: {required}"
        );
    }
    if repo.path().join("Cargo.toml").exists() {
        let rust: Vec<_> = jobs.keys().filter(|id| id.starts_with("rust-")).collect();
        assert_ne!(rust.len(), 0);
        for id in rust {
            eprintln!(
                "rust_job_{id}={:?}",
                super::support::rendered_job_block(workflow, id)?
            );
        }
    }
    Ok(())
}
