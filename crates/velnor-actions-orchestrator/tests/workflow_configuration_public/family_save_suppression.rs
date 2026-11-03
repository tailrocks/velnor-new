// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, migrate_config, prepare, render_staged_tree};

use super::bun_routing::make_bun_repo;
use super::waste_oracle::files_of;

const DISABLED: &str = "false";
const PUSH: &str = "github.event_name == 'push'";
const PROTECTED: &str = "github.ref_protected == true";

#[test]
fn family_cache_saves_are_disabled_outside_the_named_writers() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_repo()?;
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
    assert!(
        workflow.contains("if: \"false\""),
        "disabled save not rendered"
    );
    Ok(())
}
