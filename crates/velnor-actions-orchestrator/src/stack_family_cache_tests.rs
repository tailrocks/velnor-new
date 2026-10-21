use velnor_actions_contract::StepRole;
use velnor_actions_contract::config::AnsibleCollection;

use super::*;

#[test]
fn each_plan_snapshots_the_lock_that_decides_its_content() {
    let bun = bun_install("web").expect("bun");
    assert_eq!(
        bun.snapshot,
        FamilySnapshot::Files(vec!["web/bun.lock".to_owned()])
    );
    assert!(
        bun.compat.contains(&"root=web".to_owned()),
        "one unit per Bun root"
    );
    assert_eq!(
        bun_install("").expect("root").snapshot,
        FamilySnapshot::Files(vec!["bun.lock".to_owned()])
    );
    assert_eq!(
        dbt_packages("analytics/dbt")
            .expect("dbt")
            .locator
            .as_deref(),
        Some("analytics-dbt")
    );
    assert!(dbt_packages("../x").is_err());
    assert_eq!(
        python_venv("ansible/requirements.txt").snapshot,
        FamilySnapshot::Files(vec!["ansible/requirements.txt".to_owned()])
    );
}

#[test]
fn the_collection_set_is_sorted_so_declaration_order_never_changes_the_key() {
    let collection = |name: &str| AnsibleCollection {
        name: name.to_owned(),
        version: "1.2.3".to_owned(),
        sha256: "ab".repeat(32),
    };
    let one = ansible_collections(&[collection("b.two"), collection("a.one")]);
    let two = ansible_collections(&[collection("a.one"), collection("b.two")]);
    assert_eq!(one, two);
}

#[test]
fn prelude_is_a_key_step_then_restores_and_producer_steps_are_size_then_save() {
    let plans = [bun_install("web").expect("bun"), playwright_browsers()];
    let prelude = prelude(&plans).expect("prelude");
    let roles: Vec<_> = prelude.iter().map(|step| step.role).collect();
    assert_eq!(
        roles,
        [
            Some(StepRole::FamilyKeys),
            Some(StepRole::FamilyRestore),
            Some(StepRole::FamilyRestore)
        ]
    );
    let saves = after_producer(&plans[0]).expect("saves");
    let roles: Vec<_> = saves.iter().map(|step| step.role).collect();
    assert_eq!(
        roles,
        [Some(StepRole::FamilySizeCheck), Some(StepRole::FamilySave)]
    );
}
