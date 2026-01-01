//! P05-9: `plan` surfaces the required-check migration procedure.
//!
//! `RequiredCheckMigration::velnor_to_ci` is the single source of
//! truth for old/new paths and checks; this test pins that `plan`
//! prints every fact and step, with the branch-protection flip
//! marked as the external admin step the generator cannot perform.

use velnor_actions_contract::RequiredCheckMigration;
use velnor_actions_orchestrator::prepare;

use super::impl_common::{TestResult, config_with_branch, make_repo, plan_for};

#[test]
fn orch_gen_plan_surfaces_required_check_migration() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let plan = plan_for(&prep)?;
    let migration = RequiredCheckMigration::velnor_to_ci();
    assert!(
        plan.contains("Required-check migration"),
        "migration section:\n{plan}"
    );
    for fact in [
        &migration.old_workflow,
        &migration.old_check,
        &migration.new_workflow,
        &migration.new_check,
    ] {
        assert!(plan.contains(fact), "plan names {fact}:\n{plan}");
    }
    let steps = migration.steps();
    assert_eq!(steps.len(), 3, "contract procedure length");
    for step in &steps {
        assert!(plan.contains(step), "plan prints {step}:\n{plan}");
    }
    assert!(
        plan.contains("[EXTERNAL: repository admin]"),
        "admin flip marked external:\n{plan}"
    );
    Ok(())
}
