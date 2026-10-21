//! `[upgrades]` emission: opt-in, generator-owned, never a pull-request check.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo, plan_for};

const UPGRADES: &str = ".github/workflows/upgrades.yml";

fn config(upgrades: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{upgrades}")
}

#[test]
fn enabled_upgrades_emit_the_workflow_and_list_it_in_the_plan() -> TestResult {
    let repo = make_repo(&config("[upgrades]\nenabled = true\nstuck_days = 5\n"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(UPGRADES).ok_or("upgrades.yml emitted")?;
    assert!(yaml.contains("cron: 17 5 * * *"), "{yaml}");
    assert!(yaml.contains("VELNOR_STUCK_DAYS: \"5\""), "{yaml}");
    assert!(
        yaml.contains("client-id: ${{ vars.VELNOR_DEPS_APP_ID }}"),
        "{yaml}"
    );
    let actionlint = tree
        .get(".github/actionlint.yaml")
        .ok_or("actionlint config")?;
    assert!(actionlint.contains("VELNOR_DEPS_APP_ID"), "{actionlint}");
    let ci = tree.get(".github/workflows/ci.yml").ok_or("ci")?;
    assert!(
        !ci.contains("Freshness gate") && !ci.contains("velnor/upgrades"),
        "the gate is never part of CI or Required"
    );
    assert!(plan_for(&prep)?.contains(UPGRADES));
    Ok(())
}

#[test]
fn absent_or_disabled_upgrades_emit_nothing() -> TestResult {
    for table in ["", "[upgrades]\nenabled = false\n"] {
        let repo = make_repo(&config(table))?;
        let prep = prepare(repo.path())?;
        assert!(
            render_staged_tree(&prep)?.get(UPGRADES).is_none(),
            "{table:?}"
        );
        assert!(!plan_for(&prep)?.contains(UPGRADES), "{table:?}");
    }
    Ok(())
}

#[test]
fn invalid_upgrades_tables_fail_config_loading() {
    for table in [
        "[upgrades]\nenabled = true\ncooldown = \"1d\"\n",
        "[upgrades]\nenabled = true\nschedule = \"daily\"\n",
        "[upgrades]\nenabled = true\nstuck_days = 0\n",
    ] {
        let repo = make_repo(&config(table)).expect("repo");
        assert!(prepare(repo.path()).is_err(), "{table:?}");
    }
}
